"""Controller contract tests with in-memory UNO doubles; never launch Office."""

import hashlib
import importlib.util
import json
import pathlib
import sys
import tempfile
import types
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("inspection_controller", pathlib.Path(__file__).with_name("controller.py"))
CONTROLLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTROLLER)


class Property:
    def __init__(self, name, _handle, value, _state):
        self.Name, self.Value = name, value


class Enumeration:
    def __init__(self, cells):
        self.cells = iter(cells)
        self.pending = None

    def hasMoreElements(self):
        if self.pending is None:
            self.pending = next(self.cells, None)
        return self.pending is not None

    def nextElement(self):
        result, self.pending = self.pending, None
        return result


class Cell:
    def __init__(self, document, column, row, error=0):
        self.document, self.column, self.row, self.error = document, column, row, error

    def getCellAddress(self):
        return types.SimpleNamespace(Sheet=1, Column=self.column, Row=self.row)

    def getFormula(self):
        return "=1/0" if self.error else "=1+1"

    def getError(self):
        if not self.document.calculated:
            raise AssertionError("cached results were consulted")
        return self.error

    def getPropertyValue(self, name):
        if name != "FormulaResultType2":
            raise AssertionError("wrong UNO result property")
        return 1

    def getValue(self):
        return 2.0


class Document:
    def __init__(self):
        self.calculated = False
        self.calculation_calls = 0
        self.exports = []
        self.cells = [Cell(self, 2, 39, 532), Cell(self, 25, 98)]

    def isReadonly(self):
        return True

    def supportsService(self, name):
        return name == "com.sun.star.sheet.SpreadsheetDocument"

    def getSheets(self):
        def sheet(ordinal):
            return types.SimpleNamespace(queryContentCells=lambda flags: types.SimpleNamespace(
                getCells=lambda: types.SimpleNamespace(createEnumeration=lambda: Enumeration(self.cells if ordinal == 1 else []))))
        return types.SimpleNamespace(getCount=lambda: 2, getByIndex=sheet)

    def enableAutomaticCalculation(self, enabled):
        if enabled:
            raise AssertionError("calculation must be explicitly controlled")

    def calculateAll(self):
        self.calculated = True
        self.calculation_calls += 1

    def storeToURL(self, destination, properties):
        if not self.calculated:
            raise AssertionError("PDF was exported before recalculation")
        self.exports.append((destination, {p.Name: p.Value for p in properties}))
        pathlib.Path(destination).write_bytes(b"%PDF-test-double-only")

    def setModified(self, _modified):
        pass

    def close(self, _ownership):
        pass


class ControllerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="anima-controller-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.program = self.root / "installation" / "program"
        self.program.mkdir(parents=True)
        (self.program / "soffice.com").write_bytes(b"not executable")
        self.original = self.root / "original.xlsx"
        self.original.write_bytes(b"immutable original controlled Office fixture")
        (self.root / "profile").mkdir()
        self.document = Document()
        self.office_exited = False
        self.launches = []
        self.prompt = False
        self.aborted = False
        self.request = {
            "version": 1, "format": "xlsx", "source": str(self.original),
            "source_sha256": hashlib.sha256(self.original.read_bytes()).hexdigest(),
            "pdf": str(self.root / "export.pdf"), "result": str(self.root / "result.json"),
            "profile": str(self.root / "profile"), "soffice": str(self.program / "soffice.com"),
            "pipe_name": "anima_inspection_" + "a" * 32,
            "formulas": [self.identity("C40"), self.identity("Z99")],
        }

    def identity(self, address):
        return {"sheet_id": "sheet:hidden-source-id", "sheet_ordinal": 1, "address": address, "formula_sha256": "a" * 64}

    def execute(self):
        descriptor = self.root / "request.json"
        descriptor.write_text(json.dumps(self.request), encoding="utf-8")

        def popen(arguments, **options):
            self.launches.append((arguments, options))
            def finish(*_args, **_kwargs):
                self.office_exited = True
                return 0
            return types.SimpleNamespace(poll=lambda: 0 if self.office_exited else None, wait=finish, kill=finish)

        def load(_source, _target, _search, properties):
            properties = {p.Name: p.Value for p in properties}
            self.assertTrue(properties["Hidden"])
            self.assertTrue(properties["ReadOnly"])
            self.assertEqual(properties["MacroExecutionMode"], 0)
            self.assertEqual(properties["UpdateDocMode"], 0)
            if self.prompt:
                def select():
                    self.aborted = True
                continuation = types.SimpleNamespace(queryInterface=lambda _kind: types.SimpleNamespace(select=select))
                properties["InteractionHandler"].handle(types.SimpleNamespace(getContinuations=lambda: [continuation]))
            return self.document

        def terminate():
            self.office_exited = True
            return True
        desktop = types.SimpleNamespace(loadComponentFromURL=load, terminate=terminate)
        scripting = types.SimpleNamespace(setPropertyValue=lambda *_: None, commitChanges=lambda: None, getPropertyValue=lambda _: True)
        product = types.SimpleNamespace(getPropertyValue=lambda _: "26.8-test-double")
        provider = types.SimpleNamespace(createInstanceWithArguments=lambda _service, args: scripting if "Scripting" in args[0].Value else product)
        context = types.SimpleNamespace()
        resolver = types.SimpleNamespace(resolve=lambda _: context)
        def service(name, _context):
            return {"com.sun.star.configuration.ConfigurationProvider": provider, "com.sun.star.frame.Desktop": desktop, "com.sun.star.bridge.UnoUrlResolver": resolver}[name]
        context.ServiceManager = types.SimpleNamespace(createInstanceWithContext=service)
        modules = {}
        for name in ["com", "com.sun", "com.sun.star", "com.sun.star.beans", "com.sun.star.sheet", "com.sun.star.sheet.CellFlags", "com.sun.star.sheet.FormulaResult", "com.sun.star.task"]:
            modules[name] = types.ModuleType(name)
            modules[name].__path__ = []
        modules["com.sun.star.beans"].PropertyValue = Property
        modules["com.sun.star.sheet.CellFlags"].FORMULA = 16
        modules["com.sun.star.sheet.FormulaResult"].VALUE = 1
        modules["com.sun.star.sheet.FormulaResult"].STRING = 2
        modules["com.sun.star.task"].XInteractionHandler = type("XInteractionHandler", (), {})
        modules["unohelper"] = types.SimpleNamespace(Base=type("Base", (), {}))
        modules["uno"] = types.SimpleNamespace(getComponentContext=lambda: context, systemPathToFileUrl=lambda path: path, getTypeByName=lambda name: name)
        original_path = sys.path[:]
        try:
            with patch.dict(sys.modules, modules), patch.object(sys, "argv", ["controller.py", str(descriptor)]), \
                 patch.object(CONTROLLER.os, "add_dll_directory", return_value=types.SimpleNamespace(close=lambda: None), create=True), \
                 patch.object(CONTROLLER.subprocess, "CREATE_NO_WINDOW", 0x08000000, create=True), \
                 patch.object(CONTROLLER.subprocess, "Popen", side_effect=popen):
                CONTROLLER.main()
        finally:
            sys.path[:] = original_path

    def test_recalculates_hidden_and_unprinted_formulas_before_exporting_same_document(self):
        original = self.original.read_bytes()
        self.execute()
        result = json.loads((self.root / "result.json").read_text(encoding="utf-8"))
        self.assertEqual(self.original.read_bytes(), original)
        self.assertEqual(self.document.calculation_calls, 1)
        results = result["calculation"]["results"]
        self.assertEqual([r["identity"]["address"] for r in results], ["C40", "Z99"])
        self.assertEqual(results[0]["value"], {"kind": "error", "code": 532})
        self.assertEqual(results[1]["value"], {"kind": "number", "value": 2.0})
        self.assertEqual(len(self.document.exports), 1)
        filters = {p.Name: p.Value for p in self.document.exports[0][1]["FilterData"]}
        self.assertFalse(filters["IsSkipEmptyPages"])
        self.assertFalse(filters["SinglePageSheets"])
        self.assertEqual(len(self.launches), 1)
        self.assertFalse(self.launches[0][1]["shell"])
        self.assertTrue(self.office_exited)

    def test_rejects_any_import_interaction_and_retires_owned_office(self):
        self.prompt = True
        with self.assertRaises(RuntimeError):
            self.execute()
        self.assertTrue(self.aborted)
        self.assertTrue(self.office_exited)
        self.assertFalse((self.root / "result.json").exists())
        self.assertFalse(self.document.exports)

    def test_rejects_formula_inventory_mismatch_without_partial_success(self):
        self.request["formulas"] = [self.identity("C40")]
        with self.assertRaises(RuntimeError):
            self.execute()
        self.assertTrue(self.office_exited)
        self.assertFalse((self.root / "result.json").exists())
        self.assertFalse(self.document.exports)


if __name__ == "__main__":
    unittest.main()
