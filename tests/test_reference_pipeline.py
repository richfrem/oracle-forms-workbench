"""Check XML2Forms output filenames without requiring the Oracle testbed."""
import importlib.util
import os
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from urllib.parse import parse_qs, urlsplit
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/run_form.py'
spec = importlib.util.spec_from_file_location('reference_runner', SCRIPT)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

FSAL_SCRIPT = SCRIPT.with_name('launch_fsal.py')
fsal_spec = importlib.util.spec_from_file_location('fsal_runner', FSAL_SCRIPT)
fsal_runner = importlib.util.module_from_spec(fsal_spec)
fsal_spec.loader.exec_module(fsal_runner)

class OutputNames(unittest.TestCase):
    def test_output_names(self):
        for xml, binary in [('orders_fmb.xml', 'orders.fmb'), ('new_form.xml', 'new_form.fmb'), ('ORDER_FMB.xml', 'ORDER.fmb')]:
            with self.subTest(xml=xml), TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                source = root / xml
                source.write_text('<Module/>')
                (root / binary).write_bytes(b'converted')
                with patch.object(runner.subprocess, 'run', return_value=SimpleNamespace(returncode=0, stdout='Module saved as')):
                    self.assertEqual(runner.convert_xml_to_fmb(source, root), root / binary)

    def test_missing_binary_is_an_error_even_on_zero_exit(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / 'broken_fmb.xml'
            source.write_text('<Module/>')
            with patch.object(runner.subprocess, 'run', return_value=SimpleNamespace(returncode=0, stdout='No output')):
                with self.assertRaisesRegex(RuntimeError, 'did not create'):
                    runner.convert_xml_to_fmb(source, root)

class CompileConnection(unittest.TestCase):
    def test_offline_unresolved_objects_explain_connected_compilation(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fmb = root / 'sales.fmb'
            fmb.write_bytes(b'test-only')
            output = SimpleNamespace(returncode=1, stdout="identifier 'OFW_ORDERS' must be declared")
            with patch.object(runner.subprocess, 'run', return_value=output):
                with self.assertRaisesRegex(RuntimeError, 'Select database-connected compilation'):
                    runner.compile_fmb_to_fmx(fmb, root, 'none', True)

class FsalParameters(unittest.TestCase):
    def test_module_parameters_are_url_encoded(self):
        url = fsal_runner.build_fsal_url(
            'sample form.fmx',
            7001,
            'none',
            True,
            ['P_TEXT=One & Two/雪', 'P_EXPR=x=y'],
        )
        query = parse_qs(urlsplit(url).query)
        self.assertEqual(query['form'], ['sample form.fmx'])
        self.assertEqual(query['P_TEXT'], ['One & Two/雪'])
        self.assertEqual(query['P_EXPR'], ['x=y'])

    def test_module_parameters_require_a_name_and_equals_sign(self):
        with self.assertRaisesRegex(ValueError, 'NAME=VALUE'):
            fsal_runner.build_fsal_url('sample.fmx', 7001, 'none', True, ['MISSING'])

    def test_run_form_accepts_repeatable_module_parameters(self):
        args = runner.parse_cli_args(['sample.fmx', '--param', 'P_ONE=1', '--param', 'P_TWO=2'])
        self.assertEqual(args.param, ['P_ONE=1', 'P_TWO=2'])

class ParentFormStaging(unittest.TestCase):
    def test_binary_parent_chain_is_staged_with_source_case_and_without_unrelated_forms(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / 'repo'
            source = root / 'external-forms'
            extra = root / 'extra-forms'
            oracle_home = root / 'oracle-home'
            repo.mkdir()
            source.mkdir()
            extra.mkdir()
            child = source / 'CHILD.fmb'
            parent = source / 'Parent_A.fmb'
            grandparent = extra / 'Parent_B.fmb'
            unrelated = source / 'UNRELATED.fmb'
            child.write_bytes(b'Parent_A')
            parent.write_bytes(b'Parent_B')
            grandparent.write_bytes(b'base template')
            unrelated.write_bytes(b'not referenced')

            with patch.dict(os.environ, {
                'OFW_ORACLE_HOME': str(oracle_home),
                'OFW_WORK_DIR': str(oracle_home / 'ofw-work'),
                'OFW_FORMS_PATH_EXTRA': str(extra),
            }):
                compile_form = runner._compile_form()
                staged = compile_form.stage_module(child, repo)

            self.assertEqual(staged.name, 'CHILD.fmb')
            self.assertEqual((staged.parent / parent.name).read_bytes(), parent.read_bytes())
            self.assertEqual((staged.parent / grandparent.name).read_bytes(), grandparent.read_bytes())
            self.assertFalse((staged.parent / unrelated.name).exists())
            try:
                staged.parent.relative_to(oracle_home)
            except ValueError:
                self.fail('staged parent forms must remain under the configured Oracle Home')

    def test_xml_parent_filename_stages_matching_fmb(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / 'repo'
            source = root / 'external-forms'
            oracle_home = root / 'oracle-home'
            repo.mkdir()
            source.mkdir()
            child = source / 'child.xml'
            parent = source / 'Parent_Form.fmb'
            child.write_text('<Module><FormModule ParentFilename="C:\\\\Forms\\\\Parent_Form.fmb"/></Module>')
            parent.write_bytes(b'parent')

            with patch.dict(os.environ, {
                'OFW_ORACLE_HOME': str(oracle_home),
                'OFW_WORK_DIR': str(oracle_home / 'ofw-work'),
                'OFW_FORMS_PATH_EXTRA': '',
            }):
                staged = runner._compile_form().stage_module(child, repo)

            self.assertEqual((staged.parent / parent.name).read_bytes(), parent.read_bytes())

    def test_work_dir_rejects_a_repository_path_before_writing(self):
        with TemporaryDirectory() as directory:
            repo = Path(directory) / 'repo'
            repo.mkdir()
            inside = repo / 'work'
            with patch.dict(os.environ, {'OFW_WORK_DIR': str(inside)}):
                with self.assertRaisesRegex(RuntimeError, 'outside the repository'):
                    runner._compile_form().work_dir(repo)
            self.assertFalse(inside.exists())

class PreviewCompatibility(unittest.TestCase):
    def test_orders_copy_uses_oracle_schema_names_without_changing_source(self):
        import xml.etree.ElementTree as ET
        with TemporaryDirectory() as directory:
            source = Path(directory) / 'orders_fmb.xml'
            fixture = SCRIPT.parents[6] / 'samples/orders_fmb.xml'
            original = fixture.read_bytes()
            source.write_bytes(original)
            target = runner.prepare_oracle_preview_xml(source)
            self.assertNotEqual(source, target)
            self.assertEqual(source.read_bytes(), original)
            root = ET.parse(target).getroot()
            ns = {'f': 'http://xmlns.oracle.com/Forms'}
            self.assertEqual(root.get('version'), '141020000')
            parameter = root.find('.//f:ModuleParameter', ns)
            self.assertEqual(parameter.get('ParameterDataType'), 'Number')
            self.assertEqual(parameter.get('ParameterInitializeValue'), '0')
            self.assertIsNone(root.find('.//f:FutureUnknownElement', ns))
            self.assertEqual(root.find('.//f:RecordGroupColumn', ns).get('ColumnDataType'), 'Character')
            self.assertIsNotNone(root.find('.//f:AttachedLibrary', ns))
            self.assertEqual(root.find('.//f:Coordinate', ns).get('RealUnit'), 'Point')
            self.assertEqual(root.find('.//f:Canvas', ns).get('Width'), '800')

    def test_explicit_geometry_is_not_replaced_by_preview_defaults(self):
        import xml.etree.ElementTree as ET
        with TemporaryDirectory() as directory:
            source = Path(directory) / 'layout.xml'
            source.write_text('<Module version="12.2.1.19.0"><FormModule Name="F"><Coordinate RealUnit="Pixel"/><Canvas Name="C" Width="321" Height="123"/></FormModule></Module>')
            target = runner.prepare_oracle_preview_xml(source)
            root = ET.parse(target).getroot()
            self.assertEqual(root.find('.//Coordinate').get('RealUnit'), 'Pixel')
            self.assertEqual(root.find('.//Canvas').get('Width'), '321')
            self.assertEqual(root.find('.//Canvas').get('Height'), '123')

    def test_other_unknown_elements_are_preserved(self):
        with TemporaryDirectory() as directory:
            source = Path(directory) / 'other.xml'
            source.write_text('<Module version="12.2.1.19.0"><CustomBusinessNode Value="keep"/></Module>')
            target = runner.prepare_oracle_preview_xml(source)
            self.assertIn('CustomBusinessNode', target.read_text())

    def test_conflicting_aliases_fail_instead_of_discarding_values(self):
        with TemporaryDirectory() as directory:
            source = Path(directory) / 'conflict.xml'
            source.write_text('<Module><ModuleParameter DataType="Char" ParameterDataType="Number"/></Module>')
            with self.assertRaisesRegex(RuntimeError, 'Conflicting'):
                runner.prepare_oracle_preview_xml(source)

SKILLS = Path(__file__).resolve().parents[1] / 'plugins/oracle-forms/.apm/skills'


def _load_with_path(script_dir, name):
    import sys
    path = str(SKILLS / script_dir / 'scripts')
    sys.path.insert(0, path)
    try:
        spec_ = importlib.util.spec_from_file_location(name, SKILLS / script_dir / 'scripts' / (name + '.py'))
        module = importlib.util.module_from_spec(spec_)
        spec_.loader.exec_module(module)
        return module
    finally:
        sys.path.remove(path)


manifest_builder = _load_with_path('converting-oracle-artifacts', 'build_dependency_manifest')
manifest_compiler = _load_with_path('compiling-and-running-oracle-forms', 'compile_from_manifest')


def _form_xml(root_tag, **attrs):
    return f'<{root_tag} {" ".join(f"{k}=\"{v}\"" for k, v in attrs.items())}/>'


class ManifestAnalysis(unittest.TestCase):
    def _source(self, directory, files):
        root = Path(directory)
        for name, text in files.items():
            (root / name).write_text(text)
        return root

    def test_providers_precede_consumers_deterministically(self):
        lib_use = '<FormModule Name="DEPT" MenuModule="MAINMENU"><AttachedLibrary Name="UTIL_LIB"/></FormModule>'
        with TemporaryDirectory() as directory:
            source = self._source(directory, {
                'DEPT_fmb.xml': lib_use, 'EMP_fmb.xml': '<FormModule Name="EMP" ParentFilename="DEPT.fmb"/>',
                'MAINMENU_mmb.xml': '<MenuModule Name="MAINMENU"/>', 'UTIL_LIB.pll': 'binary'})
            ordered, unresolved, cycles = manifest_builder.resolve_and_order(manifest_builder.scan_source(source))
        self.assertEqual([n['name'] for n in ordered], ['MAINMENU', 'UTIL_LIB', 'DEPT', 'EMP'])
        self.assertEqual((unresolved, cycles), ([], []))

    def test_missing_provider_is_reported(self):
        with TemporaryDirectory() as directory:
            source = self._source(directory, {'DEPT_fmb.xml': '<FormModule Name="DEPT"><AttachedLibrary Name="GONE"/></FormModule>'})
            _, unresolved, _ = manifest_builder.resolve_and_order(manifest_builder.scan_source(source))
        self.assertEqual([(u['module'], u['dependency']) for u in unresolved], [('DEPT', 'GONE')])

    def test_cycle_is_detected(self):
        with TemporaryDirectory() as directory:
            source = self._source(directory, {
                'A_fmb.xml': '<FormModule ParentFilename="B.fmb"/>', 'B_fmb.xml': '<FormModule ParentFilename="A.fmb"/>'})
            ordered, _, cycles = manifest_builder.resolve_and_order(manifest_builder.scan_source(source))
        self.assertEqual(ordered, [])
        self.assertEqual(len(cycles), 1)

    def test_build_dir_inside_repo_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'inside the repository'):
            manifest_builder.require_external(SKILLS / 'x')
        with TemporaryDirectory() as directory:
            self.assertEqual(manifest_builder.require_external(Path(directory)), Path(directory).resolve())


class ManifestCompile(unittest.TestCase):
    def test_parent_module_detected_and_self_ignored(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / 'KID_fmb.xml'
            path.write_text('<FormModule Name="KID"><Block ParentModule="BASE"/><Block ParentModule="KID"/></FormModule>')
            node = manifest_builder.scan_xml(path)
        self.assertEqual(node['deps']['parentModules'], ['BASE'])

    def test_xml2forms_sets_forms_path_and_early_build_dir_check(self):
        with TemporaryDirectory() as directory:
            stage = Path(directory)
            cmd = ' '.join(manifest_builder.xml2forms_command(stage, stage / 'A_fmb.xml'))
        self.assertIn('FORMS_PATH=/work:', cmd)
        with patch.object(manifest_compiler.cf, 'container_path', side_effect=RuntimeError('nope')):
            with self.assertRaisesRegex(RuntimeError, 'not inside OFW_ORACLE_HOME'):
                manifest_compiler.require_oracle_home_build(Path('/elsewhere'))

    def test_prompts_for_missing_credentials_without_echo(self):
        with patch.object(manifest_compiler.cf, '_env', side_effect=lambda n, d='': d), \
             patch('builtins.input', side_effect=['scott', 'DEVDB']), \
             patch.object(manifest_compiler.getpass, 'getpass', return_value='pw') as password:
            self.assertEqual(manifest_compiler.resolve_userid(None, False), 'scott/pw@DEVDB')
        password.assert_called_once()
        self.assertIsNone(manifest_compiler.resolve_userid(None, True))

    def test_compiles_in_manifest_order_and_skips_dependents_of_failures(self):
        import json
        mods = [
            {'order': 1, 'name': 'OBJ', 'artifactType': 'OLB', 'dependencies': {'objectLibraries': [], 'attachedLibraries': [], 'menus': [], 'parentModules': []}},
            {'order': 2, 'name': 'LIB', 'artifactType': 'PLL', 'executableFile': 'stage/LIB.plx', 'dependencies': {'objectLibraries': [], 'attachedLibraries': [], 'menus': [], 'parentModules': []}},
            {'order': 3, 'name': 'FRM', 'artifactType': 'FMB', 'executableFile': 'stage/FRM.fmx', 'dependencies': {'objectLibraries': [], 'attachedLibraries': ['LIB'], 'menus': [], 'parentModules': []}}]
        with TemporaryDirectory() as directory:
            manifest = Path(directory) / 'build-manifest.json'
            manifest.write_text(json.dumps({'buildDir': directory, 'modules': mods}))
            calls = []
            with patch.object(manifest_compiler, 'compile_module', side_effect=lambda m, b, u: (calls.append(m['name']) or False, 'boom')):
                self.assertEqual(manifest_compiler.run_manifest(manifest, None, True), 1)
            result = {m['name']: m.get('status') for m in json.loads(manifest.read_text())['modules']}
        self.assertEqual(calls, ['LIB'])
        self.assertEqual(result['LIB'], 'failed')
        self.assertEqual(result['FRM'], 'skipped')

if __name__ == '__main__':
    unittest.main()
