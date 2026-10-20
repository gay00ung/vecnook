"""User-visible 1.3 contracts with real storage and deterministic local embeddings."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import unittest
import search
import test_search as fixture


class ExperienceTests(unittest.TestCase):
    setUp = fixture.IntegrationTests.setUp
    tearDown = fixture.IntegrationTests.tearDown

    def sync(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return search.sync_documents(self.args, self.client)

    def test_metadata_only_and_unchanged_chunks_reuse_exact_original_vectors(self):
        source = self.documents / '한글 notes 🚀.md'
        source.write_text('a'*3000+'b'*3000, encoding='utf-8')
        self.args.record_metadata = True
        self.sync()
        before = len(self.server.inputs)
        stamp = source.stat().st_mtime + 86400
        os.utime(source, (stamp, stamp))
        report = self.sync()
        self.assertEqual((report['reembed'], report['reused_vectors']), (0, 2))
        self.assertEqual(len(self.server.inputs), before)
        self.args.tags = ['updated']
        self.sync()
        self.assertEqual(len(self.server.inputs), before)
        source.write_text('c'*3000+'b'*3000, encoding='utf-8')
        report = self.sync()
        self.assertEqual((report['reembed'], report['reused_vectors']), (1, 1))
        self.assertEqual(len(self.server.inputs[-1]['input']), 1)
        rows = search.inspected_state(self.args)['documents']
        self.assertEqual(''.join(d['text'] for d in rows), 'c'*3000+'b'*3000)
        self.assertEqual({tuple(d['vector']) for d in rows}, {(1.0,0.0)})

    def test_dry_run_has_no_model_or_database_side_effects_even_with_torn_wal(self):
        class NoModel:
            def digest(self, *_):
                raise AssertionError('dry-run must not call a model')
            embed = digest
        self.args.dry_run, self.args.sync_json = True, True
        with contextlib.redirect_stdout(io.StringIO()) as output:
            report = search.sync_documents(self.args, NoModel())
        self.assertEqual(json.loads(output.getvalue())['reembed'], 1)
        self.assertIsNone(report['dimensions'])
        self.assertFalse(self.args.db.exists())
        self.args.dry_run = False
        self.sync()
        with (self.args.db/'wal.bin').open('ab') as wal:
            wal.write(b'\x01\x02\x03')
        def files():
            return {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.args.db.iterdir()}
        before = files()
        self.args.dry_run = True
        with contextlib.redirect_stdout(io.StringIO()):
            report = search.sync_documents(self.args, NoModel())
        self.assertEqual(report['current_diagnostics']['pending_tail_bytes'], 3)
        self.assertEqual(files(), before)

    def test_native_json_sync_explain_empty_results_and_model_environment(self):
        def native(*args, success=True):
            result = subprocess.run([str(self.args.binary), *map(str,args)], capture_output=True,
                                    text=True, encoding='utf-8', timeout=30)
            self.assertEqual(result.returncode == 0, success, result.stderr)
            return result
        response = native('docs-sync', self.documents, '--db', self.args.db, '--json',
                          '--ollama-url', self.client.url)
        self.assertEqual(json.loads(response.stdout)['changed'], 1)
        env = native('env-check', '--db', self.args.db, '--json', '--ollama-url', self.client.url)
        self.assertTrue(json.loads(env.stdout)['ready'])
        missing = native('env-check', '--model', 'not-installed', '--json',
                         '--ollama-url', self.client.url, success=False)
        self.assertEqual(json.loads(missing.stdout)['checks'][-1]['kind'], 'model_missing')
        empty = native('docs-find', 'missing_token', '--db', self.args.db, '--mode', 'keyword')
        self.assertIn('No results', empty.stdout)
        explained = native('docs-find', '제목', '--db', self.args.db, '--mode', 'keyword', '--explain')
        self.assertIn('explicit_mode', explained.stdout)
        self.assertIn('not confidence percentages', explained.stdout)
        failure = native('--json-errors', 'docs-get', '--db', self.args.db, '9999', success=False)
        self.assertEqual(json.loads(failure.stderr)['error']['kind'], 'InvalidInput')
        for command in ['docs-info', 'docs-list', 'docs-find', 'docs-sync']:
            self.assertIn(command.removeprefix('docs-') if command in ['docs-find','docs-sync'] else command,
                          native(command, '--help').stdout)

    def test_structural_chunker_preserves_original_text_and_reindex_keeps_old_space(self):
        source = self.documents / '한글 notes 🚀.md'
        source.write_text('# One\nfirst paragraph\n```rust\n# literal\n```\n# Two\nsecond\n', encoding='utf-8')
        chunks = search.read_chunks(self.documents, chunker='markdown-v1')
        self.assertEqual(len(chunks), 2)
        self.assertEqual(''.join(c.text for c in chunks), source.read_bytes().decode('utf-8'))
        self.sync()
        old = search.inspected_state(self.args)
        self.args.to = self.root/'reindexed'
        self.args.chunker = 'markdown-v1'
        with contextlib.redirect_stdout(io.StringIO()):
            search.reindex_documents(self.args, self.client)
        self.assertEqual(search.inspected_state(self.args), old)
        self.assertTrue(self.args.to.exists())
        self.args.to = self.args.db
        self.args.resume = True
        with self.assertRaisesRegex(ValueError, 'outside the original'):
            search.reindex_documents(self.args, self.client)
        self.assertEqual(search.inspected_state(self.args), old)

    def test_jsonl_roundtrip_large_ids_f32_bits_and_invalid_import_is_unclaimed(self):
        self.sync()
        state = search.inspected_state(self.args)
        self.args.output = self.root/'original.jsonl'
        with contextlib.redirect_stdout(io.StringIO()):
            search.export_jsonl(self.args, None)
        lines = self.args.output.read_text(encoding='utf-8').splitlines()
        row = json.loads(lines[1])
        row['id'] = str(2**64-1)
        row['vector'] = [0.12345679104328156, -0.0]
        lines[1] = json.dumps(row, ensure_ascii=False)
        self.args.input = self.root/'large-id.jsonl'
        self.args.input.write_text('\n'.join(lines)+'\n', encoding='utf-8')
        old = self.args.db
        self.args.db = self.root/'restored'
        with contextlib.redirect_stdout(io.StringIO()):
            search.import_jsonl(self.args, None)
        restored = search.inspected_state(self.args)['documents'][0]
        self.assertEqual(restored['id'], str(2**64-1))
        self.assertEqual(restored['text'], state['documents'][0]['text'])
        import struct
        self.assertEqual(struct.pack('<ff', *restored['vector']), struct.pack('<ff', *row['vector']))
        self.args.db = self.root/'invalid'
        self.args.input.write_text(lines[0]+'\n'+lines[1]+'\n'+lines[1]+'\n', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            search.import_jsonl(self.args, None)
        self.assertFalse(self.args.db.exists())
        self.args.db = old

    def test_python_client_custom_provider_and_structured_errors(self):
        package = Path(__file__).resolve().parents[2]/'clients/python/vecnook.py'
        spec = importlib.util.spec_from_file_location('vecnook_client', package)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        client = module.Client.create(self.root/'custom', 2, 'custom-model-v1', binary=self.args.binary)
        document = dict(id=str(2**64-1), text='백업을 복원합니다', source='notes.md')
        client.put_text(document, lambda texts: [[1,0] for _ in texts], 'custom-model-v1')
        result = client.search_keywords('백업', analyzer='korean-ngram')
        self.assertEqual(result['matches'][0]['document']['id'], str(2**64-1))
        self.assertEqual(client.get(document['id'], include_vector=True)['vector'], [1,0])
        with self.assertRaises(module.VecnookError) as error:
            client.put_text(document, lambda _: self.fail('mismatched provider was called'), 'wrong-model')
        self.assertEqual(error.exception.kind, 'EmbeddingMismatch')
        with self.assertRaises(module.VecnookError) as error:
            client.get('1')
        self.assertEqual(error.exception.kind, 'InvalidInput')

    def test_large_vector_query_uses_a_bounded_file_without_enabling_batch_file_reads(self):
        package = Path(__file__).resolve().parents[2]/'clients/python/vecnook.py'
        spec = importlib.util.spec_from_file_location('large_vector_client', package)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        client = module.Client.create(self.root/'large', 4096, 'large-fixture', binary=self.args.binary)
        vector = [0.123456789]*4096
        client.put(dict(id='1', source='large.md', text='original'), vector)
        result = client.search_vector(vector, k=1, strategy='exact')
        self.assertEqual(result['matches'][0]['document']['id'], '1')
        self.assertEqual(result['matches'][0]['distance'], 0.0)
        reference = self.root/'vector.csv'
        reference.write_text(','.join(map(str,vector)), encoding='utf-8')
        batch = self.root/'invalid-vector.tsv'
        payload = search.encode_document(2, search.Chunk('hidden.md',1,1,'must not insert'), [])
        batch.write_text(f'put\t2\t@{reference}\t{payload}\n', encoding='utf-8')
        with self.assertRaisesRegex(module.VecnookError, 'invalid'):
            client._run('docs-batch','--db',client.path,batch,json_result=False)
        self.assertEqual(len(client.inspect()['documents']), 1)


if __name__ == '__main__':
    unittest.main()
