#!/usr/bin/env python3
"""Extract text from a PDF file using pdfplumber."""

import argparse
import sys
from pathlib import Path

from typing import List, Optional

import pdfplumber


def extract_text(pdf_path: Path, pages: Optional[List[int]] = None) -> str:
    """Extract text from a PDF. If pages is None, extract all pages."""
    parts: list[str] = []
    with pdfplumber.open(str(pdf_path)) as pdf:
        total = len(pdf.pages)
        target = pages or list(range(1, total + 1))
        for pn in target:
            if pn < 1 or pn > total:
                print(f"Warning: page {pn} out of range (1-{total}), skipping", file=sys.stderr)
                continue
            page = pdf.pages[pn - 1]
            text = page.extract_text()
            if text:
                parts.append(text)
            else:
                parts.append(f"[Page {pn}: no extractable text]")
    return "\n\n".join(parts)


def parse_pages(raw: str) -> list[int]:
    """Parse page specs: '3' -> [3], '1-5' -> [1,2,3,4,5]."""
    raw = raw.strip()
    if "-" in raw:
        a, b = raw.split("-", 1)
        return list(range(int(a), int(b) + 1))
    return [int(raw)]


def main() -> None:
    parser = argparse.ArgumentParser(description="Extract text from a PDF file")
    parser.add_argument("path", type=Path, help="Path to the PDF file")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--page", type=int, help="Extract a single page (1-based)")
    group.add_argument("--pages", type=parse_pages, help="Extract page range (e.g. 1-5)")
    group.add_argument("--first", type=int, help="Extract only the first N pages")
    parser.add_argument("--out", type=Path, help="Write output to file instead of stdout")
    args = parser.parse_args()

    if not args.path.exists():
        print(f"Error: file not found: {args.path}", file=sys.stderr)
        sys.exit(1)

    if args.page:
        pages = [args.page]
    elif args.pages:
        pages = args.pages
    elif args.first:
        pages = list(range(1, args.first + 1))
    else:
        pages = None

    text = extract_text(args.path, pages)

    if args.out:
        args.out.write_text(text)
        print(f"Done → {args.out}")
    else:
        print(text)


if __name__ == "__main__":
    main()
