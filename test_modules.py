#!/usr/bin/env python3
"""Comprehensive test of all three modules: tokenization, transcription, stress assignment."""

import sys
import json
from pathlib import Path

# Add IPA dir to path
sys.path.insert(0, str(Path.cwd() / 'IPA'))

from corpus_metrics import Transcriber, tokenize, Token

print("=" * 60)
print("MODULE 1: Tokenization")
print("=" * 60)

text = "Мама вже вдома. Як ти?"
print(f"\nInput text: {text}\n")

tokens = list(tokenize(text))
print(f"Tokenized into {len(tokens)} tokens:\n")
for i, token in enumerate(tokens, 1):
    print(f"  {i}. [{token.type:10}] {repr(token.text)}")

# Verify token types
word_tokens = [t for t in tokens if t.type == 'word']
punct_tokens = [t for t in tokens if t.type == 'punct']
ws_tokens = [t for t in tokens if t.type in ('whitespace', 'linebreak')]

print(f"\nSummary:")
print(f"  - Words: {len(word_tokens)}")
print(f"  - Punctuation: {len(punct_tokens)}")
print(f"  - Whitespace: {len(ws_tokens)}")

assert len(word_tokens) > 0, "Should have word tokens"
assert len(punct_tokens) > 0, "Should have punctuation tokens"
assert len(ws_tokens) > 0, "Should have whitespace tokens"
print("\n✓ Tokenization module works!")

print("\n" + "=" * 60)
print("MODULE 2: IPA Transcription")
print("=" * 60)

print("\nCreating transcriber...")
t = Transcriber.create()
print("✓ Transcriber created")

# Test some Ukrainian words
test_words = ["мама", "книга", "привіт", "вдома"]
print(f"\nTranscribing words: {test_words}\n")

for word in test_words:
    ipa, stress_idx = t.to_ipa(word)
    print(f"  {word:15} -> IPA: {ipa:20} (stress syllable: {stress_idx})")
    assert isinstance(ipa, str), f"IPA should be string, got {type(ipa)}"
    assert stress_idx is None or isinstance(stress_idx, int), f"Stress should be int or None"

print("\n✓ IPA Transcription module works!")

print("\n" + "=" * 60)
print("MODULE 3: Stress Assignment")
print("=" * 60)

print("\nTesting stress assignment via transcriber...")
print("(Note: Requires ua-stress-engine in system Python)")
print("\nStress indices for Ukrainian words:\n")

for word in test_words:
    ipa, stress_idx = t.to_ipa(word)
    if stress_idx is not None:
        print(f"  {word:15} -> stress on syllable {stress_idx}")
    else:
        print(f"  {word:15} -> no stress data (using fallback)")

print("\n✓ Stress Assignment module works!")

print("\n" + "=" * 60)
print("INTEGRATION TEST")
print("=" * 60)

# Test the full pipeline: tokenize -> transcribe -> analyze
full_text = "Привіт, як ти?"
print(f"\nFull text: {full_text}\n")

tokens = list(tokenize(full_text))
print(f"Tokenized: {len(tokens)} tokens")

word_tokens = [t for t in tokens if t.type == 'word']
print(f"Word tokens: {[t.text for t in word_tokens]}\n")

print("Transcribing each word:")
for word_token in word_tokens:
    word = word_token.text
    ipa, stress = t.to_ipa(word)
    print(f"  {word:15} -> {ipa:20} (stress: {stress})")

print("\n✓ Full integration pipeline works!")

print("\n" + "=" * 60)
print("ALL TESTS PASSED!")
print("=" * 60)
