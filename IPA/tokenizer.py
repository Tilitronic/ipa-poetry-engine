"""Text tokenization for Ukrainian text into IPA stream elements."""

import re
from dataclasses import dataclass
from typing import Iterator, Literal


@dataclass
class Token:
    """A single token in the text stream.
    
    Types correspond to IpaStream elements:
    - "word": lexical word (IpaStreamWord)
    - "punct": punctuation mark (IpaStreamPunctuation)
    - "whitespace": space/tab (IpaStreamWhitespace)
    - "linebreak": newline (IpaStreamLineBreak)
    """
    type: Literal["word", "punct", "whitespace", "linebreak"]
    text: str


# Normalize punctuation to standard forms for IPA stream
PUNCT_NORMALIZATION = {
    "—": "—",  # em dash
    "–": "–",  # en dash
    "…": ".",  # ellipsis → period (pragmatic)
}


def normalize_punctuation(punct: str) -> str:
    """Normalize punctuation for IPA stream compatibility."""
    return PUNCT_NORMALIZATION.get(punct, punct)


def tokenize(text: str) -> Iterator[Token]:
    """Tokenize Ukrainian text into IPA stream elements.
    
    Preserves all punctuation (not removed), whitespace, and line structure.
    Each token corresponds to an IpaStream element type.
    
    Args:
        text: Ukrainian text to tokenize
        
    Yields:
        Token objects with type and text
    """
    # Build pattern that matches:
    # - Words: cyrillic letters, latin letters, digits, apostrophes
    # - Punctuation: common marks, dashes, brackets (kept separate per mark for precise positioning)
    # - Whitespace (including newlines)
    
    pattern = (
        r"("
        r"[\u0430-\u044F\u0456\u0457\u0454\u0491\u0410-\u042F\u0406\u0407\u0404\u0490A-Za-z0-9'ʼ-]+"  # words
        r"|[.!?,;:\-\(\)\[\]{}—–…]"  # individual punct marks (not grouped)
        r"|[\s]+"  # whitespace (can be grouped)
        r")"
    )
    
    for match in re.finditer(pattern, text, re.UNICODE):
        token_text = match.group(0)
        
        # Whitespace → linebreak or whitespace
        if re.match(r'[\s]+', token_text):
            if '\n' in token_text:
                yield Token(type='linebreak', text=token_text)
            else:
                yield Token(type='whitespace', text=token_text)
        # Punctuation → punct (including normalization)
        elif re.match(r'[.!?,;:\-\(\)\[\]{}—–…]', token_text):
            normalized = normalize_punctuation(token_text)
            yield Token(type='punct', text=normalized)
        # Everything else → word
        else:
            yield Token(type='word', text=token_text)

