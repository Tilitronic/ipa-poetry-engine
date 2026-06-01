#!/usr/bin/env python3
"""Quick test of Transcriber with stress engine."""

import sys
import json
from pathlib import Path

# Add IPA dir to path
sys.path.insert(0, str(Path.cwd() / 'IPA'))

# Import and test
from corpus_metrics import Transcriber

print("Creating transcriber...", file=sys.stderr)
t = Transcriber.create()

print("Testing transcription...", file=sys.stderr)
ipa, si = t.to_ipa('мама')
print(f'мама -> {ipa} (stress={si})')

ipa2, si2 = t.to_ipa('книга')
print(f'книга -> {ipa2} (stress={si2})')

print("\n✓ Stress engine integration successful!")
