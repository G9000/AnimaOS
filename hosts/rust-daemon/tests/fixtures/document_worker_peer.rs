//! Finite, source-controlled hostile IPC/resource peer. Never used by daemon code.
use std::io::{self, Read, Write};
use std::time::Duration;

#[cfg(windows)]
fn limits() -> io::Result<()> {
    use win_custody::{Job, JobLimits};
    let expected = JobLimits::new()
        .kill_on_close(true)
        .active_processes(1)
        .process_memory(256 * 1024 * 1024)
        .job_memory(256 * 1024 * 1024);
    let job = Job::create(&expected)?;
    if job.limits()? != expected {
        return Err(io::Error::other("limits differ"));
    }
    job.assign_current_process()
}

fn frame(
    output: &mut impl Write,
    value: &anima_core::document_content::ExtractedDocumentText,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
    output.write_all(&bytes)?;
    output.flush()
}

fn main() -> io::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "real" {
        return anima_daemon::document_worker_entry();
    }
    if mode == "neighbor" {
        std::thread::sleep(Duration::from_secs(5));
        return Ok(());
    }
    #[cfg(not(windows))]
    return Err(io::Error::other("Windows resource peer only"));
    #[cfg(windows)]
    {
        limits()?;
        if mode == "before-ready" {
            std::thread::sleep(Duration::from_secs(2));
            return Ok(());
        }
        if mode == "crash" {
            std::process::exit(7);
        }
        let mut output = io::stdout().lock();
        if mode == "bad-ready" {
            output.write_all(b"ANIMA-DOCUMENT-WRONG-v2\n")?;
            return output.flush();
        }
        if mode == "old-ready" {
            output.write_all(b"ANIMA-DOCX-READY-v1\n")?;
            return output.flush();
        }
        output.write_all(b"ANIMA-DOCUMENT-READY-v2\n")?;
        output.flush()?;
        if mode == "during-input" {
            std::thread::sleep(Duration::from_secs(2));
            return Ok(());
        }
        let mut input = io::stdin().lock();
        let mut format = [0; 1];
        input.read_exact(&mut format)?;
        use anima_core::document_content::{
            DocumentExtractionStatus, DocumentFormat, ExtractedDocumentText, PdfCoverage,
        };
        let format = match format[0] {
            0 => DocumentFormat::Docx,
            1 => DocumentFormat::Pdf,
            _ => return Err(io::Error::other("unknown request format")),
        };
        let mut header = [0; 4];
        input.read_exact(&mut header)?;
        let length = u32::from_le_bytes(header) as usize;
        if length > 2 * 1024 * 1024 {
            return Err(io::Error::other("oversize request"));
        }
        let mut payload = vec![0; length];
        input.read_exact(&mut payload)?;
        let mut eof = [0; 1];
        if input.read(&mut eof)? != 0 {
            return Err(io::Error::other("trailing request"));
        }
        match mode.as_str() {
            "old-version" | "unknown-format" => {
                let bytes = if mode == "old-version" {
                    br#"{"version":1,"status":"no_text","blocks":[],"omissions":[]}"#.as_slice()
                } else {
                    br#"{"version":2,"format":"unknown","status":"no_text","blocks":[],"omissions":[]}"#.as_slice()
                };
                output.write_all(&(bytes.len() as u32).to_le_bytes())?;
                output.write_all(bytes)?;
            }
            "oversize" => output.write_all(&(64 * 1024u32).to_le_bytes())?,
            "malformed" => {
                output.write_all(&1u32.to_le_bytes())?;
                output.write_all(b"{")?;
            }
            "stderr-flood" => {
                io::stderr().write_all(&vec![b'e'; 4097])?;
            }
            "during-output" => {
                output.write_all(&100u32.to_le_bytes())?;
                output.write_all(b"{")?;
                output.flush()?;
                std::thread::sleep(Duration::from_secs(2));
            }
            "resources" => {
                let mut oversized = Vec::<u8>::new();
                let memory_refused = oversized.try_reserve_exact(512 * 1024 * 1024).is_err();
                let child_refused = match std::process::Command::new(std::env::current_exe()?)
                    .arg("neighbor")
                    .spawn()
                {
                    Err(_) => true,
                    Ok(mut child) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        false
                    }
                };
                if !memory_refused || !child_refused {
                    return Err(io::Error::other("resource refusal missing"));
                }
                use anima_core::document_content::{
                    DocumentBlock, DocumentLocator, DocumentOmission,
                };
                frame(
                    &mut output,
                    &ExtractedDocumentText {
                        version: 2,
                        format,
                        status: DocumentExtractionStatus::Ready,
                        blocks: vec![DocumentBlock {
                            locator: match format {
                                DocumentFormat::Docx => {
                                    DocumentLocator::DocxParagraph { paragraph: 1 }
                                }
                                DocumentFormat::Pdf => DocumentLocator::PdfPage { page: 1 },
                            },
                            text: "memory-refused child-refused".into(),
                        }],
                        omissions: vec![match format {
                            DocumentFormat::Docx => DocumentOmission::MainBodyOnly,
                            DocumentFormat::Pdf => DocumentOmission::TextLayerOnly,
                        }],
                        pdf_coverage: (format == DocumentFormat::Pdf).then_some(PdfCoverage {
                            page_count: 1,
                            text_page_count: 1,
                            included_page_count: 1,
                        }),
                    },
                )?;
            }
            "valid" | "wrong-format" | "trailing" | "late" | "cpu-burn" => {
                if mode == "late" {
                    std::thread::sleep(Duration::from_secs(2));
                }
                if mode == "cpu-burn" {
                    // Finite even if a test assertion fails; parent wall-time
                    // retirement, rather than an OS CPU quota, is the witness.
                    let until = std::time::Instant::now() + Duration::from_secs(2);
                    while std::time::Instant::now() < until {
                        std::hint::spin_loop();
                    }
                }
                let format = if mode == "wrong-format" {
                    match format {
                        DocumentFormat::Docx => DocumentFormat::Pdf,
                        DocumentFormat::Pdf => DocumentFormat::Docx,
                    }
                } else {
                    format
                };
                let mut value =
                    ExtractedDocumentText::empty_for(format, DocumentExtractionStatus::NoText);
                if format == DocumentFormat::Pdf {
                    value.pdf_coverage = Some(PdfCoverage {
                        page_count: 0,
                        text_page_count: 0,
                        included_page_count: 0,
                    });
                }
                frame(&mut output, &value)?;
                if mode == "trailing" {
                    output.write_all(b"x")?;
                }
            }
            _ => return Err(io::Error::other("unknown finite fixture mode")),
        }
        output.flush()
    }
}
