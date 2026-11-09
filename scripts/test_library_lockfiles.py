"""Lockfile preparation honors independent Cargo workspace configuration."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('lockfiles', Path(__file__).with_name('library_lockfiles.py'))
lockfiles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lockfiles)


class LibraryLockfileTests(unittest.TestCase):
    def test_independent_workspace_targets_do_not_require_an_environment_override(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            example = root / 'example'
            example.mkdir()
            (root / 'Cargo.toml').write_text('[workspace]\nexclude=["example"]\n')
            (example / 'Cargo.toml').write_text('[workspace]\n')
            metadata = [json.dumps({'workspace_root': str(path), 'target_directory': str(path / 'target')})
                        for path in (root, example)]
            with patch.dict('os.environ', {}, clear=True), \
                 patch.object(lockfiles.subprocess, 'check_output', side_effect=['Cargo.toml\nexample/Cargo.toml', *metadata]), \
                 patch.object(lockfiles.subprocess, 'run') as generate:
                lockfiles.ensure_library_lockfiles(root)
            self.assertEqual(generate.call_count, 2)
            self.assertEqual({call.args[0][-1] for call in generate.call_args_list},
                             {str(root / 'Cargo.toml'), str(example / 'Cargo.toml')})


if __name__ == '__main__':
    unittest.main()
