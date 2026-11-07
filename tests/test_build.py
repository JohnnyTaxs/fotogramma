import os
import unittest
from unittest.mock import patch

from deployment.build import app_version
from fotogramma import __version__


class AppVersionTests(unittest.TestCase):
    def test_release_tag_wins(self):
        with patch.dict(os.environ, {"FOTOGRAMMA_VERSION": "v1.4.2"}, clear=False):
            self.assertEqual(app_version(), "1.4.2")

    def test_package_version_is_the_fallback(self):
        with patch.dict(os.environ, {"FOTOGRAMMA_VERSION": ""}, clear=False):
            self.assertEqual(app_version(), __version__)

    def test_non_version_values_are_ignored(self):
        with patch.dict(os.environ, {"FOTOGRAMMA_VERSION": "main"}, clear=False):
            self.assertEqual(app_version(), __version__)


if __name__ == "__main__":
    unittest.main()
