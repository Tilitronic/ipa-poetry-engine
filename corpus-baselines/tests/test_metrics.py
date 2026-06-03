import unittest

from corpus_baselines.metrics import find_best_threshold


class MetricsThresholdTests(unittest.TestCase):
    def test_finds_separating_threshold(self):
        poetry = [0.8, 0.9, 0.85]
        prose = [0.2, 0.3, 0.4]
        result = find_best_threshold(poetry, prose, "line_end_echo_ratio")
        self.assertGreaterEqual(result.balanced_accuracy, 0.99)
        self.assertEqual(result.direction, ">=")

    def test_empty_input_is_safe(self):
        result = find_best_threshold([], [], "lexical_diversity")
        self.assertEqual(result.threshold, 0.0)
        self.assertEqual(result.balanced_accuracy, 0.0)


if __name__ == "__main__":
    unittest.main()