//! Bounded JSONL transport around one pinned generation and reusable SQ8 view.
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use vecnook::{
    Error, Result,
    generation::{ScalarKernel, Snapshot},
};

const MAX_LINE: usize = 1024 * 1024;
const MAX_IDS: usize = 65_536;

fn invalid(message: &str) -> Error {
    Error::InvalidInput(message.into())
}

pub(super) fn run(args: &[String]) -> Result<()> {
    let batch = args[0] == "snapshot-batch";
    let start_options = if batch { 3 } else { 2 };
    if args.len() < start_options {
        return Err(invalid(
            "snapshot-session <store> [--sq8 N] [--idle-ms N], or snapshot-batch <store> <requests.jsonl>",
        ));
    }
    let input = if batch {
        Some(File::open(&args[2])?)
    } else {
        None
    };
    let mut candidates = None;
    let mut cache = None;
    let mut cache_options = vecnook::generation::QuantizedCacheOptions::default();
    let mut idle_ms = 60_000u64;
    let mut seen = BTreeSet::new();
    let mut position = start_options;
    while position < args.len() {
        let option = args[position].as_str();
        if !seen.insert(option) || position + 1 == args.len() {
            return Err(invalid("duplicate or incomplete session option"));
        }
        match option {
            "--sq8" => {
                let value: usize = super::parse(&args[position + 1], "SQ8 candidates")?;
                if !(1..=65_536).contains(&value) {
                    return Err(invalid("SQ8 candidates must be 1..=65536"));
                }
                candidates = Some(value);
            }
            "--idle-ms" => {
                idle_ms = super::parse(&args[position + 1], "idle milliseconds")?;
                if !(1..=86_400_000).contains(&idle_ms) {
                    return Err(invalid("idle milliseconds must be 1..=86400000"));
                }
            }
            "--sq8-cache" => cache = Some(args[position + 1].clone()),
            "--buffer-bytes" => {
                cache_options = cache_options
                    .with_buffer_bytes(super::parse(&args[position + 1], "SQ8 buffer bytes")?)
            }
            _ => return Err(invalid("unknown session option")),
        }
        position += 2;
    }
    if (cache.is_some() && candidates.is_none())
        || (cache.is_none() && seen.contains("--buffer-bytes"))
    {
        return Err(invalid("cache requires --sq8; buffer requires --sq8-cache"));
    }
    let start = Instant::now();
    let snapshot = Snapshot::open(&args[1])?;
    let open_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let quantized = if cache.is_none() {
        candidates.map(|_| snapshot.quantize()).transpose()?
    } else {
        None
    };
    let build_ms = if quantized.is_some() {
        start.elapsed().as_secs_f64() * 1000.
    } else {
        0.
    };
    let start = Instant::now();
    let cached = cache
        .as_ref()
        .map(|path| snapshot.quantize_cached(path, cache_options))
        .transpose()?;
    let cache_ms = if cached.is_some() {
        start.elapsed().as_secs_f64() * 1000.
    } else {
        0.
    };
    // Counters describe completed constructor calls, outside the request loop.
    let opens = 1;
    let builds = usize::from(quantized.is_some());
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "{{\"schema_version\":1,\"event\":\"ready\",\"generation\":\"{}\",\"sequence\":\"{}\",\"snapshot_opens\":{opens},\"sq8_builds\":{builds},\"cache_hits\":{},\"code_read_buffer_bytes\":{},\"open_ms\":{open_ms},\"code_build_ms\":{build_ms},\"cache_validation_load_ms\":{cache_ms}}}",
        snapshot.generation(),
        snapshot.sequence(),
        usize::from(cached.is_some()),
        cached.as_ref().map_or(0, |v| v.buffer_bytes())
    )?;
    output.flush()?;
    // A rendezvous channel leaves at most one bounded frame waiting in the
    // producer. It never accumulates a queue proportional to the input stream.
    let (send, receive) = mpsc::sync_channel(0);
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader: Box<dyn BufRead> = match input {
            Some(file) => Box::new(BufReader::new(file)),
            None => Box::new(stdin.lock()),
        };
        produce_frames(reader.as_mut(), send);
    });
    loop {
        let line = match receive.recv_timeout(Duration::from_millis(idle_ms)) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                writeln!(
                    output,
                    "{{\"schema_version\":1,\"event\":\"closed\",\"reason\":\"eof\"}}"
                )?;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                writeln!(
                    output,
                    "{{\"schema_version\":1,\"event\":\"idle_timeout\"}}"
                )?;
                break;
            }
        };
        let request = line.and_then(|line| Parser::new(&line).request());
        match request {
            Err(error @ Error::Io(_)) => {
                emit_error(&mut output, None, &error)?;
                output.flush()?;
                return Err(error);
            }
            Err(error) => emit_error(&mut output, None, &error)?,
            Ok(request) if request.close => {
                writeln!(
                    output,
                    "{{\"schema_version\":1,\"event\":\"closed\",\"request_id\":{}}}",
                    super::json_string(&request.id)
                )?;
                break;
            }
            Ok(request) => {
                let started = Instant::now();
                let result = if let (Some(view), Some(limit)) = (&cached, candidates) {
                    match &request.ids {
                        Some(ids) => {
                            view.search_ids(&request.vector, request.k, limit, &ScalarKernel, ids)
                        }
                        None => view.search(&request.vector, request.k, limit),
                    }
                } else {
                    match (&quantized, candidates, &request.ids) {
                        (Some(view), Some(limit), Some(ids)) => {
                            view.search_ids(&request.vector, request.k, limit, &ScalarKernel, ids)
                        }
                        (Some(view), Some(limit), None) => {
                            view.search(&request.vector, request.k, limit)
                        }
                        _ => {
                            let ids = request
                                .ids
                                .map(|ids| ids.into_iter().collect::<BTreeSet<_>>());
                            snapshot.search_filtered(
                                &request.vector,
                                request.k,
                                &ScalarKernel,
                                |id, _| ids.as_ref().is_none_or(|ids| ids.contains(&id)),
                            )
                        }
                    }
                };
                match result {
                    Ok(report) => writeln!(
                        output,
                        "{{\"schema_version\":1,\"request_id\":{},\"snapshot_opens\":{opens},\"sq8_builds\":{builds},\"search_ms\":{},\"result\":{}}}",
                        super::json_string(&request.id),
                        started.elapsed().as_secs_f64() * 1000.,
                        super::generation_json(&report)
                    )?,
                    Err(error) => emit_error(&mut output, Some(&request.id), &error)?,
                }
            }
        }
        output.flush()?;
    }
    output.flush()?;
    Ok(())
}

fn produce_frames(reader: &mut dyn BufRead, send: mpsc::SyncSender<Result<Vec<u8>>>) {
    loop {
        let result = frame(reader);
        if matches!(&result, Ok(None)) {
            break;
        }
        let fatal = matches!(&result, Err(Error::Io(_)));
        if send.send(result.map(|line| line.unwrap())).is_err() || fatal {
            break;
        }
    }
}

fn emit_error(output: &mut impl Write, id: Option<&str>, error: &Error) -> Result<()> {
    writeln!(
        output,
        "{{\"schema_version\":1,\"request_id\":{},\"error\":{{\"kind\":{},\"message\":{}}}}}",
        id.map_or_else(|| "null".into(), super::json_string),
        super::json_string(&format!("{:?}", error.kind())),
        super::json_string(&error.to_string())
    )?;
    Ok(())
}

fn frame(reader: &mut dyn BufRead) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    if reader
        .take((MAX_LINE + 1) as u64)
        .read_until(b'\n', &mut line)?
        == 0
    {
        return Ok(None);
    }
    if line.len() > MAX_LINE {
        // Drain this frame without retaining its tail; preserve the next frame.
        if line.last() != Some(&b'\n') {
            loop {
                let bytes = reader.fill_buf()?;
                if bytes.is_empty() {
                    break;
                }
                let end = bytes.iter().position(|b| *b == b'\n');
                let count = end.map_or(bytes.len(), |end| end + 1);
                reader.consume(count);
                if end.is_some() {
                    break;
                }
            }
        }
        return Err(invalid("session request exceeds 1 MiB"));
    }
    Ok(Some(line))
}

#[derive(Debug)]
struct Request {
    id: String,
    vector: Vec<f32>,
    k: usize,
    ids: Option<Vec<u64>>,
    close: bool,
}

// This protocol has no recursive JSON values. Bound both the frame and each
// collection before allocation, and reject unknown/duplicate fields.
struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn whitespace(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.position += 1;
        }
    }
    fn consume(&mut self, byte: u8) -> bool {
        self.whitespace();
        if self.bytes.get(self.position) == Some(&byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, byte: u8) -> Result<()> {
        if self.consume(byte) {
            Ok(())
        } else {
            Err(invalid("invalid session JSON structure"))
        }
    }
    fn hex(&mut self) -> Result<u32> {
        let bytes = self
            .bytes
            .get(self.position..self.position + 4)
            .ok_or_else(|| invalid("truncated JSON escape"))?;
        if !bytes.iter().all(u8::is_ascii_hexdigit) {
            return Err(invalid("invalid JSON hex escape"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("invalid JSON escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| invalid("invalid JSON escape"))?;
        self.position += 4;
        Ok(value)
    }
    fn string(&mut self, maximum: usize) -> Result<String> {
        self.expect(b'"')?;
        let mut value = Vec::new();
        loop {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or_else(|| invalid("unterminated JSON string"))?;
            self.position += 1;
            match byte {
                b'"' => {
                    return String::from_utf8(value).map_err(|_| invalid("invalid UTF-8 string"));
                }
                0..=31 => return Err(invalid("unescaped JSON control character")),
                b'\\' => {
                    let escaped = *self
                        .bytes
                        .get(self.position)
                        .ok_or_else(|| invalid("truncated JSON escape"))?;
                    self.position += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => value.push(escaped),
                        b'b' => value.push(8),
                        b'f' => value.push(12),
                        b'n' => value.push(b'\n'),
                        b'r' => value.push(b'\r'),
                        b't' => value.push(b'\t'),
                        b'u' => {
                            let mut scalar = self.hex()?;
                            if (0xd800..=0xdbff).contains(&scalar) {
                                if self.bytes.get(self.position..self.position + 2) != Some(b"\\u")
                                {
                                    return Err(invalid("missing low surrogate"));
                                }
                                self.position += 2;
                                let low = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err(invalid("invalid low surrogate"));
                                }
                                scalar = 0x10000 + ((scalar - 0xd800) << 10) + low - 0xdc00;
                            }
                            let scalar = char::from_u32(scalar)
                                .ok_or_else(|| invalid("invalid Unicode scalar"))?;
                            let mut encoded = [0; 4];
                            value.extend_from_slice(scalar.encode_utf8(&mut encoded).as_bytes());
                        }
                        _ => return Err(invalid("invalid JSON escape")),
                    }
                }
                _ => value.push(byte),
            }
            if value.len() > maximum {
                return Err(invalid("session string exceeds bound"));
            }
        }
    }
    fn number(&mut self) -> Result<&'a str> {
        self.whitespace();
        let start = self.position;
        if self.bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        match self.bytes.get(self.position) {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                while self
                    .bytes
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
            }
            _ => return Err(invalid("expected JSON number")),
        }
        for marker in *b".e" {
            let found = self.bytes.get(self.position).copied();
            if found == Some(marker) || (marker == b'e' && found == Some(b'E')) {
                self.position += 1;
                if marker == b'e'
                    && self
                        .bytes
                        .get(self.position)
                        .is_some_and(|b| matches!(b, b'+' | b'-'))
                {
                    self.position += 1;
                }
                let digits = self.position;
                while self
                    .bytes
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
                if digits == self.position {
                    return Err(invalid("truncated JSON number"));
                }
            }
        }
        std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| invalid("invalid JSON number"))
    }
    fn request(mut self) -> Result<Request> {
        let (mut id, mut vector, mut ids, mut k, mut close) = (None, None, None, 10, false);
        let mut fields = BTreeSet::new();
        self.expect(b'{')?;
        if !self.consume(b'}') {
            loop {
                let field = self.string(32)?;
                if !fields.insert(field.clone()) {
                    return Err(invalid("duplicate session field"));
                }
                self.expect(b':')?;
                match field.as_str() {
                    "request_id" => id = Some(self.string(128)?),
                    "op" => {
                        close = match self.string(16)?.as_str() {
                            "search" => false,
                            "close" => true,
                            _ => return Err(invalid("unknown session operation")),
                        }
                    }
                    "k" => {
                        k = self
                            .number()?
                            .parse::<usize>()
                            .map_err(|_| invalid("K must be a nonnegative integer"))?;
                    }
                    "vector" => {
                        let mut values = Vec::new();
                        self.expect(b'[')?;
                        if !self.consume(b']') {
                            loop {
                                if values.len() == 4096 {
                                    return Err(invalid("too many vector coordinates"));
                                }
                                let value = self
                                    .number()?
                                    .parse::<f32>()
                                    .map_err(|_| invalid("invalid coordinate"))?;
                                if !value.is_finite() {
                                    return Err(invalid("non-finite coordinate"));
                                }
                                values.push(value);
                                if self.consume(b']') {
                                    break;
                                }
                                self.expect(b',')?;
                            }
                        }
                        vector = Some(values);
                    }
                    "ids" => {
                        let mut values = Vec::new();
                        self.expect(b'[')?;
                        if !self.consume(b']') {
                            loop {
                                if values.len() == MAX_IDS {
                                    return Err(invalid("eligible ID list exceeds65536"));
                                }
                                let id = self.string(20)?;
                                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                                    return Err(invalid("ID must be a decimal u64 string"));
                                }
                                values.push(
                                    id.parse::<u64>()
                                        .map_err(|_| invalid("ID must be a decimal u64 string"))?,
                                );
                                if self.consume(b']') {
                                    break;
                                }
                                self.expect(b',')?;
                            }
                        }
                        ids = Some(values);
                    }
                    _ => return Err(invalid("unknown session field")),
                }
                if self.consume(b'}') {
                    break;
                }
                self.expect(b',')?;
            }
        }
        self.whitespace();
        if self.position != self.bytes.len() {
            return Err(invalid("trailing session JSON"));
        }
        let id = id.ok_or_else(|| invalid("request_id is required"))?;
        if close && (vector.is_some() || ids.is_some() || fields.contains("k")) {
            return Err(invalid("close cannot contain search fields"));
        }
        let vector = if close {
            Vec::new()
        } else {
            vector.ok_or_else(|| invalid("vector is required"))?
        };
        Ok(Request {
            id,
            vector,
            k,
            ids,
            close,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_json_preserves_unicode_signed_zero_and_full_u64() {
        let request = Parser::new(
            br#"{"request_id":"\ud83d\ude00","vector":[-0.0,1e-2],"ids":["18446744073709551615"]}"#,
        )
        .request()
        .unwrap();
        assert_eq!(request.id, "😀");
        assert_eq!(request.vector[0].to_bits(), (-0.0f32).to_bits());
        assert_eq!(request.ids, Some(vec![u64::MAX]));
        for bytes in [
            br#"{"request_id":"x","vector":[01]}"#.as_slice(),
            br#"{"request_id":"x","vector":[1e400]}"#,
            br#"{"request_id":"x","vector":[NaN]}"#,
            br#"{"request_id":"x","request_id":"y","vector":[1]}"#,
            br#"{"request_id":"\ud800","vector":[1]}"#,
            br#"{"request_id":"\u+123","vector":[1]}"#,
            br#"{"request_id":"x","vector":[1],"ids":["+1"]}"#,
            br#"{"request_id":"x","vector":[1],}"#,
            br#"{"request_id":"x","op":"close","k":1}"#,
        ] {
            assert!(Parser::new(bytes).request().is_err(), "{bytes:?}");
        }
    }
    #[test]
    fn oversized_frame_is_drained_without_losing_next_request() {
        let mut bytes = vec![b' '; MAX_LINE + 1];
        bytes.extend_from_slice(b"tail\n{\"request_id\":\"next\",\"op\":\"close\"}\n");
        let mut input = io::Cursor::new(bytes);
        assert!(frame(&mut input).is_err());
        assert_eq!(
            Parser::new(&frame(&mut input).unwrap().unwrap())
                .request()
                .unwrap()
                .id,
            "next"
        );
    }
    #[test]
    fn permanent_input_io_failure_emits_once_and_stops() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("input failed"))
            }
        }
        let (send, receive) = mpsc::sync_channel(0);
        let worker = thread::spawn(move || produce_frames(&mut BufReader::new(Broken), send));
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err(Error::Io(_))
        ));
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        worker.join().unwrap();
    }
}
