#!/usr/bin/env python3
"""Predeclared authored retrieval cases with genuine local embeddings; not user research."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'examples/documents'))
from search import Ollama, Chunk, cli, coordinates, document_input, encode_document, query_input, retrieval_mode

# Topics and labels are authored before any model output is measured.
TOPICS = [
    ('backup', '독립 백업 만들기', 'create an independent backup', 'An independent backup is copied to a new directory and verified by reopening the copy. 백업은 새로운 디렉터리에 만들고 복사본을 열어 검증합니다.'),
    ('recovery', '프로세스 종료 후 성공한 쓰기 복구', 'recover acknowledged writes after a process crash', 'A WAL acknowledgement follows synchronization. Complete frames replay after process restart. 프로세스가 종료돼도 성공 응답을 받은 쓰기는 WAL 재생으로 복구합니다.'),
    ('checkpoint', '체크포인트로 재시작 재생 비용 줄이기', 'bound WAL replay work with a checkpoint', 'Checkpoint writes a snapshot and clears the WAL. It bounds replay work but does not remove tombstones. 체크포인트는 스냅샷을 저장하고 WAL을 비워 재시작 비용을 줄입니다.'),
    ('compact', '삭제 노드 공간 회수', 'reclaim deleted nodes with compaction', 'Compaction rebuilds active records and removes tombstones. 압축은 삭제된 노드와 이전 버전을 제거해 공간을 회수합니다.'),
    ('lock', '다른 프로세스가 연 데이터베이스의 잠금', 'handle a database already owned by another process', 'An exclusive OS lock prevents another persistent handle. 데이터베이스가 이미 열려 있으면 소유한 핸들을 닫거나 그 핸들로 요청해야 합니다.'),
    ('identity', '검색 질문과 문서의 임베딩 모델 일치', 'match query and document embedding identities', 'Model digest, dimensions, metric and prompts must match. 문서와 검색 질문의 모델 버전과 차원이 달라지면 새 컬렉션에 다시 색인합니다.'),
    ('cosine', '코사인 거리와 영벡터 제약', 'interpret cosine distance and zero vectors', 'Cosine distance is one minus similarity and rejects zero vectors. 코사인 거리는 낮을수록 가깝고 영벡터는 허용하지 않습니다.'),
    ('exact', '정확 검색으로 근사 검색 재현율 검증', 'measure ANN recall against exact search', 'Exact search scans eligible original vectors and supplies a reference ranking. 정확 검색 결과와 근사 검색을 비교해 재현율을 계산합니다.'),
    ('ef', '탐색 폭과 검색 지연의 관계', 'trade search breadth for recall and latency', 'Increasing graph exploration usually improves recall at more distance-computation cost. 탐색 폭을 높이면 보통 재현율과 계산량 및 지연이 함께 증가합니다.'),
    ('batch', '여러 쓰기의 원자적 반영', 'apply several mutations atomically', 'An atomic batch validates all members and persists one synced WAL frame. 배치는 여러 변경을 하나의 WAL 프레임으로 반영하며 실패 시 일부만 반영하지 않습니다.'),
    ('filter', '태그와 날짜 조건으로 검색 범위 제한', 'restrict retrieval by tag and date filters', 'AND OR NOT and typed numeric/date bounds select eligible documents before ranking. 태그와 숫자 및 날짜 범위를 결합해 검색 대상을 제한합니다.'),
    ('capacity', '차원 수를 고려한 스냅샷 용량 계산', 'estimate snapshot capacity from dimensions and text', 'Snapshot bytes and physical node limits both apply. 고차원 벡터와 원문은 스냅샷 용량을 사용하며 개수 한계보다 먼저 바이트 한계에 도달할 수 있습니다.'),
    ('korean', '한국어 조사와 키워드 토큰 처리', 'match Korean word particles in lexical search', 'The optional Korean analyzer uses Hangul character grams. 한국어 키워드의 조사가 달라지는 경우 선택형 분석기로 부분 단어를 검색할 수 있습니다.'),
    ('hybrid', '의미 검색과 키워드 검색 순위 결합', 'combine semantic and lexical ranks', 'Reciprocal rank fusion combines vector and BM25 candidate ranks. 하이브리드 검색은 벡터 검색과 키워드 검색의 순위를 결합하지만 항상 관련성이 좋아지지는 않습니다.'),
    ('sync', '문서 변경 시 임베딩 재사용', 'reuse unchanged document embeddings during sync', 'Only changed embedding inputs require new model computation. 문서의 날짜나 태그만 바뀌면 기존 벡터를 재사용하고 바뀐 청크만 다시 임베딩합니다.'),
    ('ids', '자바스크립트에서 큰 문서 식별자 보존', 'preserve large document identifiers in JavaScript', 'Full u64 identifiers travel as decimal strings rather than JavaScript numbers. 큰 문서 ID는 JSON 문자열로 전달하고 Number로 변환하지 않습니다.'),
    ('jsonl', '다른 도구로 원문과 벡터 내보내기', 'transfer original records through JSONL', 'JSONL exchanges original text and coordinates with canonical string IDs. 원문과 벡터를 JSONL로 교환하며 검증 후 새 경로로 복원합니다.'),
    ('queue', '사용자 화면을 멈추지 않는 작업 큐', 'keep a UI responsive with a bounded worker queue', 'A background worker owns the collection and receives nonblocking submissions. 작업 큐는 백그라운드에서 검색하며 UI 스레드에서는 응답을 기다려 차단하지 않습니다.'),
    ('errors', '쓰기 중 입출력 오류 후 재시도 판단', 'reconcile an I/O error before retrying a write', 'An I/O failure may occur after a write persisted. 쓰기 오류가 반환돼도 저장됐을 수 있으므로 다시 열어 해당 ID를 확인한 뒤 재시도를 결정합니다.'),
    ('context', '검색 청크 주변의 원문 문맥 읽기', 'read neighboring original chunks', 'Context expansion returns adjacent chunks from the same source while retaining eligibility filters. 검색 결과 주변 문맥은 같은 출처의 인접 청크이며 필터로 제외된 문서는 포함하지 않습니다.'),
]


def dataset():
    chunks = [Chunk(name+'.md', 1, 1, text) for name, ko, en, text in TOPICS]
    questions = []
    for index, (name, ko, en, _) in enumerate(TOPICS):
        for pattern in ['{} 방법은?', '{}에 관해 설명해주세요', '{}를 하려면 어떻게 해야 하나요?', '{} 관련 동작이 궁금합니다', '{}를 확인하고 싶어요']:
            questions.append(dict(language='ko', kind='meaning', query=pattern.format(ko), expected=name+'.md'))
        for pattern in ['How can I {}?', 'Explain how to {}.'] + (['What should I know to {}?'] if index < 10 else []):
            questions.append(dict(language='en', kind='meaning', query=pattern.format(en), expected=name+'.md'))
    for code in range(50):
        token = f'ERR_RETRIEVAL_{code:03}'
        source = f'code-{code:03}.md'
        chunks.append(Chunk(source, 1, 1, f'{token}: A fictional application operation failed; inspect its recorded request before retrying. 가상 오류 코드 {token}의 요청을 확인합니다.'))
        questions.append(dict(language='identifier', kind='identifier', query=token, expected=source))
    for index in range(30):
        questions.append(dict(language='none', kind='unanswerable', query=f'UNAVAILABLE_ASTRONOMY_{index:03}', expected=None))
    for index, row in enumerate(questions):
        row['split'] = 'validation' if index % 5 == 0 else 'development'
    return chunks, questions


def metrics(rows):
    answerable = [r for r in rows if r['expected'] is not None]
    ranks = [r['rank'] for r in answerable]
    return dict(queries=len(rows), answerable=len(ranks), hit_at_1=sum(r == 1 for r in ranks)/len(ranks),
                mrr_at_10=sum(1/r if r else 0 for r in ranks)/len(ranks),
                ndcg_at_10=sum(1/math.log2(r+1) if r else 0 for r in ranks)/len(ranks),
                source_recall_at_10=sum(r is not None for r in ranks)/len(ranks),
                unanswerable_returned=sum(bool(r['returned']) for r in rows if r['expected'] is None))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', default='target/release/vecnook')
    parser.add_argument('--baseline-binary')
    parser.add_argument('--model', default='embeddinggemma:latest')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    chunks, questions = dataset()
    judgments = json.dumps(questions, ensure_ascii=False, sort_keys=True).encode()
    (root/'judgments.json').write_bytes(judgments)
    model = Ollama()
    digest = model.digest(args.model)
    inputs = [document_input(args.model,c) for c in chunks]+[query_input(args.model,q['query']) for q in questions]
    vectors = []
    for at in range(0, len(inputs), 16):
        vectors += model.embed(args.model, inputs[at:at+16])
    vectors = [[struct.unpack('<f',struct.pack('<f',v))[0] for v in row] for row in vectors]
    if digest != model.digest(args.model):
        raise ValueError('embedding identity changed during evaluation')
    identity = json.dumps(dict(provider='ollama',model=args.model,digest=digest,prompt_version=1),separators=(',',':'))
    space = [root, 'corpus', len(vectors[0]), identity, 'cosine']
    cli(args.binary, 'docs-init', *space)
    batch = root/'input.tsv'
    batch.write_text(''.join(f'put\t{i}\t{coordinates(v)}\t{encode_document(i,c,[])}\n' for i,(c,v) in enumerate(zip(chunks,vectors))),encoding='utf-8')
    cli(args.binary,'docs-batch',*space,batch)
    cli(args.binary,'docs-checkpoint',*space)
    rows = []
    for q, vector in zip(questions, vectors[len(chunks):]):
        for mode in ['vector','keyword','hybrid','auto','keyword-korean']:
            selected = retrieval_mode(q['query'],mode) if mode != 'keyword-korean' else 'keyword'
            if selected == 'vector':
                command = ['docs-search',*space,coordinates(vector),10,128,'exact']
            elif selected == 'keyword':
                command = ['docs-keywords',*space,q['query'],10]
            else:
                command = ['docs-hybrid',*space,coordinates(vector),q['query'],10]
            if mode == 'keyword-korean':
                command += ['--analyzer','korean-ngram']
            start = time.perf_counter()
            response = json.loads(cli(args.binary,*command))
            elapsed = (time.perf_counter()-start)*1000
            if args.baseline_binary and mode != 'keyword-korean':
                baseline = json.loads(cli(args.baseline_binary,*command))
                if baseline != response:
                    raise ValueError('default response regression: '+q['query'])
            sources = [r['document']['source'] for r in response['matches']]
            rows.append(dict(q, mode=mode, selected_mode=selected, returned=sources,
                             rank=sources.index(q['expected'])+1 if q['expected'] in sources else None, cli_ms=elapsed))
    summary = {mode: {split: metrics([r for r in rows if r['mode']==mode and (split=='all' or r['split']==split)])
                      for split in ['all','development','validation']}
               for mode in ['vector','keyword','hybrid','auto','keyword-korean']}
    report = dict(schema_version=1, model=args.model, model_digest=digest, dimensions=len(vectors[0]),
                  documents=len(chunks), questions=len(questions), judgments_sha256=hashlib.sha256(judgments).hexdigest(),
                  vectors_sha256=hashlib.sha256(b''.join(struct.pack('<f',v) for row in vectors for v in row)).hexdigest(),
                  binary_sha256=hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),
                  identical_default_baseline_responses=bool(args.baseline_binary), metrics=summary, rows=rows,
                  scope='Authored bilingual topic descriptions and templated questions: 100 Korean, 50 English, 50 identifiers, 30 unanswerable. No user data or independent relevance judgments; no calibration on validation results. Vector search returns nearest candidates even for unanswerable questions; scores are not confidence.')
    (root/'results.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(summary,ensure_ascii=False))


if __name__ == '__main__':
    main()
