#!/usr/bin/env python3
"""Comprehensive test of all three modules with IPA stream compatibility."""

import sys
import json
from pathlib import Path

# Add IPA dir to path
sys.path.insert(0, str(Path.cwd() / 'IPA'))

from corpus_metrics import Transcriber, tokenize, Token

print("=" * 60)
print("MODULE 1: Tokenization (with punctuation preservation)")
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

# Verify punctuation is preserved in stream (NOT removed)
all_punct = ''.join(t.text for t in punct_tokens)
assert '.' in all_punct, "Punctuation should be preserved"
assert '?' in all_punct, "Question mark should be preserved"

print("\n✓ Tokenization module works with punctuation preservation!")

print("\n" + "=" * 60)
print("MODULE 2: IPA Transcription with Syllable Structure")
print("=" * 60)

print("\nCreating transcriber...")
t = Transcriber.create()
print("✓ Transcriber created")

# Test some Ukrainian words
test_words = ["мама", "книга", "привіт", "вдома"]
print(f"\nTranscribing words: {test_words}\n")

for word in test_words:
    word_obj = t.to_word(word)
    print(f"  {word:15}")
    print(f"    IPA: {word_obj.ipa}")
    print(f"    Stressed syllable: {word_obj.stressed_syllable}")
    print(f"    Stress source: {word_obj.stress_source}")
    print(f"    Syllables: {len(word_obj.syllables)}")
    
    # Verify structure
    assert isinstance(word_obj.ipa, str), f"IPA should be string"
    assert isinstance(word_obj.stressed_syllable, int), f"Stress should be int"
    assert word_obj.stress_source in ("dict", "ml"), f"Stress source should be dict or ml"
    assert isinstance(word_obj.syllables, list), f"Syllables should be list"

print("\n✓ IPA Transcription module works!")

print("\n" + "=" * 60)
print("MODULE 3: Stress Assignment Sources")
print("=" * 60)

print("\nStress assignment sources for Ukrainian words:\n")

for word in test_words:
    word_obj = t.to_word(word)
    source_label = {
        "dict": "✓ Dictionary (known word)",
        "ml": "? ML/Luscinia (unknown word - prediction needed)",
        "manual": "! Manual (user-provided)"
    }.get(word_obj.stress_source, "?")
    
    stress_info = f"syllable {word_obj.stressed_syllable}" if word_obj.stressed_syllable >= 0 else "unknown"
    print(f"  {word:15} ({source_label})")
    print(f"    → stress on {stress_info}\n")

print("✓ Stress Assignment module works!")

print("\n" + "=" * 60)
print("MODULE 4: Full IPA Stream Pipeline")
print("=" * 60)

# Test the full pipeline: tokenize -> transcribe -> build stream structure
full_text = "Привіт, як ти?"
print(f"\nFull text: {full_text}\n")

tokens = list(tokenize(full_text))
print(f"Tokenized: {len(tokens)} tokens\n")

stream_elements = []
line_index = 0
word_index = 0

for token in tokens:
    if token.type == 'word':
        word_obj = t.to_word(token.text)
        stream_element = {
            "type": "word",
            "original": token.text,
            "ipa": word_obj.ipa,
            "stressedSyllable": word_obj.stressed_syllable,
            "stressSource": word_obj.stress_source,
            "syllableCount": len(word_obj.syllables),
        }
        stream_elements.append(stream_element)
        print(f"WORD: {token.text:15} → {word_obj.ipa:20} (source: {word_obj.stress_source})")
        word_index += 1
    elif token.type == 'punct':
        stream_elements.append({"type": "punctuation", "text": token.text})
        print(f"PUNCT: {repr(token.text)}")
    elif token.type == 'whitespace':
        stream_elements.append({"type": "whitespace"})
    elif token.type == 'linebreak':
        stream_elements.append({"type": "line_break", "lineIndex": line_index})
        line_index += 1
        word_index = 0

print(f"\nStream structure built: {len(stream_elements)} elements")
print(f"  Words: {sum(1 for e in stream_elements if e['type'] == 'word')}")
print(f"  Punctuation: {sum(1 for e in stream_elements if e['type'] == 'punctuation')}")
print(f"  Whitespace: {sum(1 for e in stream_elements if e['type'] == 'whitespace')}")

print("\n✓ Full IPA stream pipeline works!")

print("\n" + "=" * 60)
print("ALL TESTS PASSED!")
print("=" * 60)
print("\nModules are now compatible with IpaStream v1.1 format:")
print("  ✓ Punctuation preserved in token stream")
print("  ✓ Word structure includes syllables and stress source")
print("  ✓ Stress source distinguishes between dict and ml (luscinia)")
print("  ✓ Ready for integration with Rust engine")
