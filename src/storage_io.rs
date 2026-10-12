//! Standard-library I/O with test-only, thread-local failure points.
use std::{
    fs::File,
    io::{self, Write},
};

pub(crate) fn check(_point: &'static str) -> io::Result<()> {
    #[cfg(test)]
    if faults::take(_point).is_some() {
        return Err(io::Error::other("injected storage failure"));
    }
    Ok(())
}
pub(crate) fn write(file: &mut File, bytes: &[u8], _point: &'static str) -> io::Result<()> {
    #[cfg(test)]
    if let Some(mode) = faults::take(_point) {
        match mode {
            faults::Mode::Partial(n) => {
                file.write_all(&bytes[..n.min(bytes.len())])?;
                return Err(io::Error::other("injected partial write"));
            }
            faults::Mode::Fail => return Err(io::Error::other("injected write failure")),
            faults::Mode::Short | faults::Mode::Interrupted => {
                struct Fragmented<'a> {
                    file: &'a mut File,
                    interrupt: bool,
                }
                impl Write for Fragmented<'_> {
                    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                        if self.interrupt {
                            self.interrupt = false;
                            return Err(io::ErrorKind::Interrupted.into());
                        }
                        self.file.write(&data[..data.len().min(3)])
                    }
                    fn flush(&mut self) -> io::Result<()> {
                        self.file.flush()
                    }
                }
                return Fragmented {
                    file,
                    interrupt: matches!(mode, faults::Mode::Interrupted),
                }
                .write_all(bytes);
            }
        }
    }
    file.write_all(bytes)
}
pub(crate) fn copy(source: &mut File, target: &mut File) -> io::Result<u64> {
    #[cfg(test)]
    if let Some(mode) = faults::take("backup.copy") {
        if let faults::Mode::Partial(n) = mode {
            io::copy(&mut std::io::Read::take(source, n as u64), target)?;
        }
        return Err(io::Error::other("injected copy failure"));
    }
    io::copy(source, target)
}

#[cfg(test)]
pub(crate) mod faults {
    use std::cell::RefCell;
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Mode {
        Fail,
        Partial(usize),
        Short,
        Interrupted,
    }
    thread_local! {static RULE:RefCell<Option<(&'static str,Mode)>>=const {RefCell::new(None)};}
    pub(crate) fn take(point: &str) -> Option<Mode> {
        RULE.with(|r| {
            let mut r = r.borrow_mut();
            if r.as_ref().is_some_and(|(p, _)| *p == point) {
                r.take().map(|(_, m)| m)
            } else {
                None
            }
        })
    }
    pub(crate) struct Guard;
    impl Guard {
        pub(crate) fn new(point: &'static str, mode: Mode) -> Self {
            RULE.with(|r| {
                assert!(r.borrow().is_none());
                *r.borrow_mut() = Some((point, mode));
            });
            Self
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            RULE.with(|r| *r.borrow_mut() = None);
        }
    }
}
