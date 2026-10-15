"""Search-mode selection contracts independent of a model or database."""
import unittest
from search import retrieval_mode


class RetrievalTests(unittest.TestCase):
    def test_identifiers_choose_hybrid(self):
        for query in ["write_batch_if_sequence", "Error::Locked 오류", "ERR-42 retry",
                      "ERR_IO_27 재시도", "AcmeSync 동기화", "Qdrant", "physical_nodes 한도"]:
            self.assertEqual(retrieval_mode(query, "auto"), "hybrid", query)

    def test_prose_and_semantic_ef_questions_keep_vector(self):
        for query in ["How do I recover my documents?", "삭제된 문서를 복구하는 방법은?",
                      "How does increasing efSearch affect recall?", "efSearch를 높이면?"]:
            self.assertEqual(retrieval_mode(query, "auto"), "vector", query)

    def test_explicit_mode_always_wins(self):
        for mode in ["vector", "keyword", "hybrid"]:
            for query in ["ERR-42", "How do I recover documents?"]:
                self.assertEqual(retrieval_mode(query, mode), mode)
