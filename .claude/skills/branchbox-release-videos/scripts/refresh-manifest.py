#!/usr/bin/env python3
"""Refresh hashes after reviewing intentional source/capture edits, not to accept unknown assets."""
import hashlib, json
from pathlib import Path
root=Path(__file__).resolve().parents[1]
p=root/'assets/manifest.json';manifest=json.loads(p.read_text());manifest['hashes']={str(f.relative_to(root)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted((root/'assets').rglob('*')) if f.is_file() and f!=p};p.write_text(json.dumps(manifest,indent=2)+'\n');print(f"Recorded {len(manifest['hashes'])} reviewed asset hashes")
