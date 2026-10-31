"""Fixed private UNO controller. Never load a script or command from a document."""

import hashlib
import json
import math
import os
import pathlib
import subprocess
import sys
import time


MAX_PDF = 2 * 1024 * 1024
MAX_RESULT = 1024 * 1024
MAX_FORMULA = 4097
MAX_TEXT = 4096


def require(condition):
    if not condition:
        raise RuntimeError("inspection incomplete")


def office_system_path(path):
    value = str(path)
    if value.startswith("\\\\?\\"):
        value = value[4:]
        require(not value.upper().startswith("UNC\\"))
    return value


def source_hash(path):
    with open(path, "rb") as stream:
        data = stream.read(4 * 1024 * 1024 + 1)
    require(0 < len(data) <= 4 * 1024 * 1024)
    return hashlib.sha256(data).hexdigest()


def cell_name(column, row):
    name = ""
    column += 1
    while column:
        column, digit = divmod(column - 1, 26)
        name = chr(65 + digit) + name
    return name + str(row + 1)


def main():
    require(len(sys.argv) == 2)
    descriptor = pathlib.Path(sys.argv[1]).resolve(strict=True)
    root = descriptor.parent
    require(descriptor.name == "request.json")
    require(descriptor.stat().st_size <= MAX_RESULT)
    with open(descriptor, "r", encoding="utf-8") as stream:
        request = json.load(stream)
    require(set(request) == {"version", "format", "source", "source_sha256", "pdf", "result", "profile", "soffice", "pipe_name", "formulas"})
    require(request["version"] == 1)
    require(request["format"] in ("docx", "xlsx", "pptx"))
    owned = {}
    for key, name in (("source", "original." + request["format"]), ("pdf", "export.pdf"), ("result", "result.json"), ("profile", "profile")):
        path = pathlib.Path(request[key])
        require(path.is_absolute() and path.parent.resolve(strict=True) == root and path.name == name)
        owned[key] = path
    require(not owned["pdf"].exists() and not owned["result"].exists())
    require(owned["profile"].is_dir())
    require(source_hash(owned["source"]) == request["source_sha256"])
    pipe_name = request["pipe_name"]
    require(isinstance(pipe_name, str) and pipe_name.startswith("anima_inspection_") and len(pipe_name) == 49)
    require(all(c in "0123456789abcdef" for c in pipe_name[17:]))
    soffice = pathlib.Path(request["soffice"]).resolve(strict=True)
    require(soffice.is_absolute() and soffice.name == "soffice.com" and soffice.parent.name == "program")
    program = soffice.parent

    # The parent supplies only pinned stdlib/program paths, disables site hooks,
    # and excludes the script/cwd from automatic module search.
    sys.dont_write_bytecode = True
    dll_directory = os.add_dll_directory(str(program))
    sys.path.insert(0, str(program))
    import uno
    import unohelper
    from com.sun.star.beans import PropertyValue
    from com.sun.star.sheet.CellFlags import FORMULA
    from com.sun.star.sheet.FormulaResult import VALUE, STRING
    from com.sun.star.task import XInteractionHandler

    def prop(name, value):
        return PropertyValue(name, 0, value, 0)

    class RejectInteraction(unohelper.Base, XInteractionHandler):
        def __init__(self):
            self.used = False

        def handle(self, interaction):
            self.used = True
            abort_type = uno.getTypeByName("com.sun.star.task.XInteractionAbort")
            for continuation in interaction.getContinuations():
                abort = continuation.queryInterface(abort_type)
                if abort is not None:
                    abort.select()
                    return

    # Every listener is a newly spawned, owned process with a fresh unique profile.
    # The enclosing checked Windows Job owns this process and all its descendants.
    arguments = [office_system_path(soffice), "--headless", "--invisible", "--nologo", "--nodefault", "--norestore", "--nofirststartwizard", "--nocrashreport",
                 "-env:UserInstallation=" + uno.systemPathToFileUrl(office_system_path(owned["profile"])),
                 "--accept=pipe,name=" + pipe_name + ";urp;StarOffice.ComponentContext"]
    office = subprocess.Popen(arguments, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              close_fds=True, creationflags=subprocess.CREATE_NO_WINDOW, cwd=office_system_path(root), shell=False)
    desktop = None
    document = None
    completed = False
    try:
        local_context = uno.getComponentContext()
        resolver = local_context.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local_context)
        endpoint = "uno:pipe,name=" + pipe_name + ";urp;StarOffice.ComponentContext"
        connection_deadline = time.monotonic() + 15.0
        context = None
        while context is None:
            require(office.poll() is None and time.monotonic() < connection_deadline)
            try:
                context = resolver.resolve(endpoint)
            except Exception:
                time.sleep(0.025)
        services = context.ServiceManager
        provider = services.createInstanceWithContext("com.sun.star.configuration.ConfigurationProvider", context)
        scripting = provider.createInstanceWithArguments("com.sun.star.configuration.ConfigurationUpdateAccess",
                                                        (prop("nodepath", "/org.openoffice.Office.Common/Security/Scripting"),))
        scripting.setPropertyValue("DisableMacrosExecution", True)
        scripting.commitChanges()
        require(scripting.getPropertyValue("DisableMacrosExecution") is True)
        product = provider.createInstanceWithArguments("com.sun.star.configuration.ConfigurationAccess",
                                                      (prop("nodepath", "/org.openoffice.Setup/Product"),))
        engine_version = product.getPropertyValue("ooSetupVersionAboutBox")
        require(isinstance(engine_version, str) and 0 < len(engine_version) <= 64 and not any(ord(c) < 32 for c in engine_version))
        desktop = services.createInstanceWithContext("com.sun.star.frame.Desktop", context)
        interaction = RejectInteraction()
        load_properties = (prop("Hidden", True), prop("ReadOnly", True), prop("AsTemplate", False),
                           prop("MacroExecutionMode", 0), prop("UpdateDocMode", 0),
                           prop("InteractionHandler", interaction))
        document = desktop.loadComponentFromURL(uno.systemPathToFileUrl(office_system_path(owned["source"])), "_blank", 0, load_properties)
        require(document is not None and not interaction.used and document.isReadonly())
        expected_service = {"docx": "com.sun.star.text.TextDocument", "xlsx": "com.sun.star.sheet.SpreadsheetDocument", "pptx": "com.sun.star.presentation.PresentationDocument"}[request["format"]]
        require(document.supportsService(expected_service))

        if request["format"] == "xlsx":
            sheets = document.getSheets()
            expected = {}
            for identity in request["formulas"]:
                require(set(identity) == {"sheet_id", "sheet_ordinal", "address", "formula_sha256"})
                ordinal = identity["sheet_ordinal"]
                require(type(ordinal) is int and 0 <= ordinal < sheets.getCount())
                key = (ordinal, identity["address"])
                require(key not in expected)
                expected[key] = identity
            # Force independent computation in this document, ignoring imported caches.
            document.enableAutomaticCalculation(False)
            document.calculateAll()
            evaluated = {}
            for ordinal in range(sheets.getCount()):
                sheet = sheets.getByIndex(ordinal)
                formula_cells = sheet.queryContentCells(FORMULA).getCells().createEnumeration()
                while formula_cells.hasMoreElements():
                    cell = formula_cells.nextElement()
                    address = cell.getCellAddress()
                    key = (address.Sheet, cell_name(address.Column, address.Row))
                    require(address.Sheet == ordinal and key in expected and key not in evaluated)
                    engine_formula = cell.getFormula()
                    require(isinstance(engine_formula, str) and engine_formula.startswith("=") and len(engine_formula.encode("utf-8")) <= MAX_FORMULA)
                    error = cell.getError()
                    if error:
                        require(0 < error <= 65535)
                        value = {"kind": "error", "code": error}
                    else:
                        result_type = cell.getPropertyValue("FormulaResultType2")
                        if result_type == VALUE:
                            number = cell.getValue()
                            require(math.isfinite(number))
                            value = {"kind": "number", "value": number}
                        elif result_type == STRING:
                            text = cell.getString()
                            require(len(text.encode("utf-8")) <= MAX_TEXT)
                            value = {"kind": "text", "value": text}
                        else:
                            raise RuntimeError("unrecognized computed value")
                    evaluated[key] = {"identity": expected[key], "engine_formula": engine_formula, "value": value}
            require(set(evaluated) == set(expected))
            calculation = {"kind": "recalculated", "results": [evaluated[(i["sheet_ordinal"], i["address"])] for i in request["formulas"]]} if expected else {"kind": "no_formulas"}
        else:
            require(not request["formulas"])
            calculation = {"kind": "not_applicable"}
        require(not interaction.used)
        export_filter = {"docx": "writer_pdf_Export", "xlsx": "calc_pdf_Export", "pptx": "impress_pdf_Export"}[request["format"]]
        filter_data = (prop("IsSkipEmptyPages", False), prop("SkipEmptyPages", False),
                       prop("SinglePageSheets", False), prop("ExportHiddenSlides", False),
                       prop("ExportNotesPages", False), prop("ExportOnlyNotesPages", False),
                       prop("ExportNotes", False), prop("UseLosslessCompression", True),
                       prop("ReduceImageResolution", False))
        # Export this same recalculated in-memory document. Never save the Office source.
        document.storeToURL(uno.systemPathToFileUrl(office_system_path(owned["pdf"])),
                            (prop("FilterName", export_filter), prop("FilterData", filter_data), prop("Overwrite", False), prop("InteractionHandler", interaction)))
        require(not interaction.used and 0 < owned["pdf"].stat().st_size <= MAX_PDF)
        require(source_hash(owned["source"]) == request["source_sha256"])
        document.setModified(False)
        document.close(True)
        document = None
        require(desktop.terminate())
        desktop = None
        require(office.wait(timeout=5.0) == 0)
        result = {"version": 1, "source_sha256": request["source_sha256"], "engine_version": engine_version, "calculation": calculation}
        encoded = json.dumps(result, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode("utf-8")
        require(0 < len(encoded) <= MAX_RESULT)
        with open(owned["result"], "xb") as stream:
            stream.write(encoded)
            stream.flush()
        completed = True
    finally:
        if document is not None:
            try:
                document.setModified(False)
                document.close(True)
            except Exception:
                pass
        if desktop is not None:
            try:
                desktop.terminate()
            except Exception:
                pass
        if office.poll() is None:
            office.kill()
            office.wait(timeout=2.0)
        dll_directory.close()
    require(completed)


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        # No exception text, document data, or paths are copied into daemon output.
        sys.exit(1)
