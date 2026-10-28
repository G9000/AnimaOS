"""Deterministic inert Office fixtures. All content is synthetic; no real credentials."""
from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_STORED

ROOT = Path(__file__).parent
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
P = "http://schemas.openxmlformats.org/presentationml/2006/main"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
REL = "http://schemas.openxmlformats.org/package/2006/relationships"
def types(path, kind):
    return f'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/{path}" ContentType="application/vnd.openxmlformats-officedocument.{kind}+xml"/></Types>'
def save(name, entries):
    with ZipFile(ROOT / name, "w", compression=ZIP_STORED) as archive:
        for path, text in entries.items():
            info = ZipInfo(path, (1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o100600 << 16
            archive.writestr(info, text.encode("utf-8"))
def word(secret):
    return {
        "[Content_Types].xml": types("word/document.xml", "wordprocessingml.document.main"),
        "_rels/.rels": f'<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>',
        "word/document.xml": f'<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>Public body</w:t></w:r></w:p></w:body></w:document>',
        "word/header1.xml": f'<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>{secret}</w:t></w:r></w:p></w:hdr>',
    }
def presentation(secret):
    return {
        "[Content_Types].xml": types("ppt/presentation.xml", "presentationml.presentation.main").replace("</Types>", '<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>'),
        "_rels/.rels": f'<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="ppt/presentation.xml"/></Relationships>',
        "ppt/presentation.xml": f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="s1"/></p:sldIdLst></p:presentation>',
        "ppt/_rels/presentation.xml.rels": f'<Relationships xmlns="{REL}"><Relationship Id="s1" Type="{R}/slide" Target="slides/slide1.xml"/></Relationships>',
        "ppt/slides/slide1.xml": f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/></p:nvSpPr><p:txBody><a:p><a:r><a:t>Public slide</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>',
        "ppt/notesSlides/notesSlide1.xml": f'<p:notes xmlns:p="{P}" xmlns:a="{A}"><a:p><a:r><a:t>{secret}</a:t></a:r></a:p></p:notes>',
    }
save("header-credential.docx", word("password=synthetic-fixture"))
save("header-safe.docx", word("Public retained heading"))
save("notes-credential.pptx", presentation("access_token=synthetic-fixture"))
save("notes-safe.pptx", presentation("Public retained notes"))
