import importlib.util
from pathlib import Path
import unittest


MODULE_PATH = Path(__file__).with_name('sign_independent.py')
SPEC = importlib.util.spec_from_file_location('sign_independent', MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class IndependentEntitlementsTests(unittest.TestCase):
    def test_local_hardened_runtime_can_load_adhoc_electron_frameworks(self):
        entitlements = MODULE.INDEPENDENT_ENTITLEMENTS
        self.assertTrue(entitlements['com.apple.security.cs.disable-library-validation'])
        self.assertTrue(entitlements['com.apple.security.cs.allow-jit'])
        self.assertNotIn('com.apple.security.app-sandbox', entitlements)


if __name__ == '__main__':
    unittest.main()
