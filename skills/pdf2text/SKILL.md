# pdf2text

_Updated: 2026-05-19_

Converts PDF files to plain text using `pdfplumber`.

## When to use

- Reading the content of a PDF file
- Extracting text from a PDF for analysis, search, or summarization
- When the user asks you to read a PDF

## Script

`python3 skills/pdf2text/pdf2text.py <path> [options]`

### Options

| Flag | Description |
|------|-------------|
| `--page N` | Extract only page N (1-based) |
| `--pages N-M` | Extract pages N through M (inclusive) |
| `--out FILE` | Write output to FILE instead of stdout |
| `--first N` | Extract only the first N pages |

### Examples

```bash
python3 skills/pdf2text/pdf2text.py document.pdf
python3 skills/pdf2text/pdf2text.py document.pdf --page 3
python3 skills/pdf2text/pdf2text.py document.pdf --pages 1-5 --out output.txt
python3 skills/pdf2text/pdf2text.py document.pdf --first 2
```
