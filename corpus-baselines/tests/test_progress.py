import io
import unittest

from corpus_baselines.progress import ProgressReporter


class ProgressTests(unittest.TestCase):
    def test_bar_renders_counts(self):
        buf = io.StringIO()
        reporter = ProgressReporter(stream=buf, width=10)
        self.assertEqual(reporter.bar(0, 5), "[..........] 0/5")
        self.assertEqual(reporter.bar(3, 5), "[######....] 3/5")
        self.assertEqual(reporter.bar(5, 5), "[##########] 5/5")

    def test_category_messages(self):
        buf = io.StringIO()
        reporter = ProgressReporter(stream=buf, width=10)
        reporter.category_start("news", 12)
        reporter.file_done("news", 1, 12, "doc_001.txt")
        reporter.category_done("news", 12.3)
        text = buf.getvalue()
        self.assertIn("[news]", text)
        self.assertIn("doc_001.txt", text)
        self.assertIn("done in 12.3s", text)


if __name__ == "__main__":
    unittest.main()
