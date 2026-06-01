{
"structurality": {
"rhythm": { "rawSignal": 0.82, "baseline": 0.5, "score": 0.64 },
"localPhonemePatterning": { "rawSignal": 0.44, "baseline": 0.18, "score": 0.32 },
"soundSequencePatterning": { "rawSignal": 0.58, "baseline": 0.1, "score": 0.53 },
"pausePatterning": { "rawSignal": 0.49, "baseline": 0.2, "score": 0.36 },
"crossLevelCoupling": { "rawSignal": 0.61, "baseline": 0.25, "score": 0.48 },
"global": 0.49,
"weights": {
"rhythm": 0.25,
"localPhonemePatterning": 0.125,
"soundSequencePatterning": 0.375,
"pausePatterning": 0.125,
"crossLevelCoupling": 0.125
},
"interdependencyModel": "pairwise_line_agreement_v1"
},
"lines": [
{
"line_index": 0,
"syllable_count": 8,
"cv_ratio": 1.25,
"words": [
{
"word_id": "w_0",
"syllables": [
{
"syl_idx": 0,
"is_stressed": false,
"is_word_last": false,
"rhyme_data": null,
"tokens": [
{
"token_idx": 0,
"ipa": "b",
"type": "consonant",
"opacity": 0.3,
"svg_connections": []
},
{
"token_idx": 1,
"ipa": "a",
"type": "vowel",
"opacity": 1.0,
"svg_connections": ["line_2_word_4_syl_0_tok_1"]
}
]
},
{
"syl_idx": 1,
"is_stressed": true,
"is_word_last": true,
"rhyme_data": {
"type": "near",
"partner_id": "line_0_word_1_syl_1"
},
"tokens": [
{ "token_idx": 2, "ipa": "ʃ", "type": "consonant", "opacity": 0.8, "svg_connections": [] },
{ "token_idx": 3, "ipa": "u", "type": "vowel", "opacity": 1.0, "svg_connections": [] },
{ "token_idx": 4, "ipa": "k", "type": "consonant", "opacity": 0.5, "svg_connections": [] }
]
}
]
}
]
}
],
"rhyme_skeleton": [
["[u k]", "[u ʃ]"],
[],
["[u k]", "[i n a]"]
]
}
