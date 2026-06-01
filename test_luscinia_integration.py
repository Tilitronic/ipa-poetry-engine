"""Test Luscinia integration with two-tier stress assignment.

Verifies:
1. LusciniaResolver model loading and stress prediction
2. Transcriber two-tier pipeline (dict fallback → ml)
3. Stress source tracking (dict vs ml)
4. Syllable structure generation
"""

import sys
from pathlib import Path

# Add IPA module to path
sys.path.insert(0, str(Path(__file__).parent))

from IPA.transcriber import Transcriber, LusciniaResolver, Word


def test_luscinia_model_loading():
    """Test that Luscinia model loads correctly."""
    print("\n=== Test 1: Luscinia Model Loading ===")
    resolver = LusciniaResolver()
    
    if resolver.model:
        print("✓ Luscinia model loaded successfully")
        print(f"  Model type: {type(resolver.model)}")
    else:
        print("✗ Luscinia model not loaded (model file not found)")
        print(f"  Expected path: w:/Projects/poetykaAnalizerEngine/ua-stress-engine/src/stress_prediction/lightgbm/artifacts/luscinia-lgbm-str-ua-univ-v1/P3_0017_FINAL_FULLDATA/P3_0017_full.lgb")
    
    if resolver.feature_builder:
        print(f"✓ Feature builder loaded: {resolver.feature_builder.__module__}.{resolver.feature_builder.__name__}")
    else:
        print("✗ Feature builder not loaded (will use placeholder features)")


def test_luscinia_predictions():
    """Test Luscinia stress predictions for known OOV words."""
    print("\n=== Test 2: Luscinia Stress Predictions ===")
    resolver = LusciniaResolver()
    
    if not resolver.model:
        print("⊘ Skipping: Luscinia model not loaded")
        return
    
    # Test cases: (word, expected_syllable_pattern)
    test_words = [
        ("невідомий", "unknown"),  # OOV word: unknown stress pattern
        ("комп'ютер", "unknown"),  # OOV word: borrowed term
        ("дивергенція", "unknown"),  # OOV word: technical term
    ]
    
    for word, _ in test_words:
        try:
            stress_idx = resolver.predict_stress(word, pos="NOUN")
            if stress_idx is not None:
                print(f"✓ '{word}' → stress at syllable {stress_idx}")
            else:
                print(f"? '{word}' → no prediction")
        except Exception as e:
            print(f"✗ '{word}' → error: {e}")


def test_transcriber_two_tier():
    """Test Transcriber two-tier pipeline (dict → ml)."""
    print("\n=== Test 3: Transcriber Two-Tier Pipeline ===")
    transcriber = Transcriber()
    
    # Test words that should use ML (dict not available in workspace)
    test_words = [
        "мама",
        "книга",
        "невідомий",
        "комп'ютер",
    ]
    
    for word in test_words:
        word_obj = transcriber.to_word(word)
        print(f"\n'{word}':")
        print(f"  IPA: {word_obj.ipa}")
        print(f"  Stress: syllable {word_obj.stressed_syllable if word_obj.stressed_syllable >= 0 else 'unknown'}")
        print(f"  Source: {word_obj.stress_source}")
        print(f"  Syllables: {len(word_obj.syllables)}")


def test_stress_source_tracking():
    """Test that stress_source field is properly populated."""
    print("\n=== Test 4: Stress Source Tracking ===")
    transcriber = Transcriber()
    
    words = ["мама", "книга", "тестувати"]
    
    for word in words:
        word_obj = transcriber.to_word(word)
        
        # Verify stress_source is set correctly
        if word_obj.stress_source not in ["dict", "ml", "manual"]:
            print(f"✗ '{word}' has invalid stress_source: {word_obj.stress_source}")
        else:
            print(f"✓ '{word}' → stress_source='{word_obj.stress_source}'")
            
            # Verify confidence mapping
            if word_obj.stress_source == "dict":
                confidence = 0.95
            elif word_obj.stress_source == "ml":
                confidence = 0.75 if transcriber.luscinia.model else 0.50
            elif word_obj.stress_source == "manual":
                confidence = 1.0
            else:
                confidence = 0.0
            
            print(f"             confidence={confidence}")


def test_syllable_structure():
    """Test that Word objects have proper syllable structures."""
    print("\n=== Test 5: Syllable Structure ===")
    transcriber = Transcriber()
    
    word = "книга"
    word_obj = transcriber.to_word(word)
    
    print(f"Word: {word_obj.original}")
    print(f"IPA: {word_obj.ipa}")
    print(f"Stressed syllable: {word_obj.stressed_syllable}")
    print(f"Number of syllables: {len(word_obj.syllables)}")
    
    for i, syllable in enumerate(word_obj.syllables):
        marked = " [STRESSED]" if i == word_obj.stressed_syllable else ""
        print(f"  {i}: {syllable.ipa} (tokens={syllable.tokens}){marked}")


def test_pipeline_integration():
    """Test full pipeline: tokenize → transcribe → stress."""
    print("\n=== Test 6: Full Pipeline Integration ===")
    
    from IPA.tokenizer import tokenize
    from IPA.transcriber import Transcriber
    
    text = "Мама вже вдома. Як ти?"
    transcriber = Transcriber()
    
    print(f"Input: {text}")
    print(f"Tokens:")
    
    for token in tokenize(text):
        if token.type == "word":
            word_obj = transcriber.to_word(token.text)
            print(f"  [{token.type}] '{token.text}' → IPA='{word_obj.ipa}', stress={word_obj.stressed_syllable}, source={word_obj.stress_source}")
        else:
            print(f"  [{token.type}] '{repr(token.text)}'")


if __name__ == "__main__":
    print("=" * 60)
    print("Luscinia Integration Tests")
    print("=" * 60)
    
    try:
        test_luscinia_model_loading()
        test_luscinia_predictions()
        test_transcriber_two_tier()
        test_stress_source_tracking()
        test_syllable_structure()
        test_pipeline_integration()
    except Exception as e:
        print(f"\n✗ Test failed with error: {e}")
        import traceback
        traceback.print_exc()
    
    print("\n" + "=" * 60)
    print("Tests complete")
    print("=" * 60)
