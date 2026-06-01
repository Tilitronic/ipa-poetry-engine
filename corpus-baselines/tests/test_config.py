import unittest

from corpus_baselines.config import get_default_source_plan, recommended_ubertext_variant, ubertext_download_url, wikisource_raw_url


class ConfigTests(unittest.TestCase):
    def test_recommended_variant_is_sentenced(self):
        self.assertEqual(recommended_ubertext_variant("news"), "sentenced")
        self.assertEqual(recommended_ubertext_variant("fiction"), "sentenced")

    def test_download_url_shape(self):
        url = ubertext_download_url("news", "sentenced")
        self.assertTrue(url.startswith("https://"))
        self.assertIn("/news/sentenced/", url)
        self.assertTrue(url.endswith("ubertext.news.filter_rus_gcld+short.text_only.txt.bz2"))

    def test_default_plan_is_prose_only(self):
        plan = get_default_source_plan()
        categories = [src.category for src in plan]
        groups = [src.group for src in plan]
        self.assertEqual(categories[:3], ["news", "wikipedia", "fiction"])
        self.assertEqual(groups[:3], ["prose", "prose", "prose"])
        self.assertEqual(groups[3:], ["poetry"] * 3)

    def test_wikisource_raw_url(self):
        url = wikisource_raw_url("Ой там орав мужик")
        self.assertTrue(url.startswith("https://uk.wikisource.org/w/index.php?title="))
        self.assertIn("&action=raw", url)

    def test_plan_has_stable_filenames(self):
        plan = get_default_source_plan()
        filenames = [src.filename for src in plan]
        self.assertEqual(filenames[0], "ubertext_news_sentenced.txt.bz2")
        self.assertEqual(filenames[1], "ubertext_wikipedia_sentenced.txt.bz2")
        self.assertTrue(filenames[-1].startswith("poetry_"))
        self.assertEqual(len(set(filenames)), len(filenames))


if __name__ == "__main__":
    unittest.main()
