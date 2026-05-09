Read [](file:///w%3A/Projects/poetykaAnalizerEngine/VersaSenseEngine/VerseSenseFrontend/src/services/phonetic/ipaStreamExport.ts#1-1), lines 1 to 260

Ось повна специфікація формату:

---

# IPA Stream — специфікація формату v1.1

## Верхній рівень

```json
{
  "metadata": { ... },
  "stream":   [ ... ]
}
```

---

## `metadata`

| Поле                 | Тип        | Опис                                                    |
| -------------------- | ---------- | ------------------------------------------------------- |
| `version`            | `"1.1"`    | Версія формату (фіксований рядок)                       |
| `generatedAt`        | `string`   | ISO 8601 UTC timestamp (`"2026-05-08T14:22:00.000Z"`)   |
| `confirmedLineCount` | `number`   | Кількість підтверджених рядків у потоці                 |
| `totalWordCount`     | `number`   | Загальна кількість слів у потоці                        |
| `languagesPresent`   | `string[]` | Відсортований список мовних кодів (напр. `["pl","uk"]`) |

---

## `stream` — масив елементів

Три можливі типи елементів, discriminated union по полю `type`:

---

### `type: "word"` — слово

| Поле               | Тип                              | Опис                                                                                  |
| ------------------ | -------------------------------- | ------------------------------------------------------------------------------------- |
| `type`             | `"word"`                         | Дискримінатор                                                                         |
| `id`               | `string`                         | Стабільний ID токена — використовуй для round-trip відповіді бекенду                  |
| `lineIndex`        | `number`                         | 0-based індекс підтвердженого рядка                                                   |
| `wordIndex`        | `number`                         | 0-based позиція слова в рядку                                                         |
| `language`         | `string`                         | Мовний код: `"uk"`, `"pl"`, `"en-us"`, `"en-gb"`                                      |
| `original`         | `string`                         | Оригінальний текст слова з вірша                                                      |
| `syllableCount`    | `number`                         | Загальна кількість складів                                                            |
| `stressedSyllable` | `number`                         | 0-based індекс наголошеного складу; `-1` якщо наголос відсутній (частки, прийменники) |
| `stressSource`     | `"dict"` \| `"ml"` \| `"manual"` | Джерело наголосу (див. нижче)                                                         |
| `syllables`        | `IpaStreamSyllable[]`            | Масив складів у порядку вимови                                                        |

#### `stressSource` — значення

| Значення   | Джерело                                  | Надійність |
| ---------- | ---------------------------------------- | ---------- |
| `"dict"`   | WASM-словник (ua-word-stress-wasm / pl)  | Висока     |
| `"ml"`     | Luscinia ONNX ML-предиктор (OOV слова)   | Середня    |
| `"manual"` | Вручну підтверджено/змінено користувачем | Абсолютна  |

---

### `IpaStreamSyllable` — склад

| Поле       | Тип        | Опис                                                                      |
| ---------- | ---------- | ------------------------------------------------------------------------- |
| `ipa`      | `string`   | Повний IPA рядок складу, напр. `"ʃuk"`                                    |
| `tokens`   | `string[]` | Дискретні фонеми у порядку, напр. `["ʃ","u","k"]`                         |
| `grapheme` | `string`   | Оригінальні букви, вирівняні до цього складу (G2P alignment, best-effort) |
| `stressed` | `boolean`  | `true` на наголошеному складі (рівно один на слово, або жодного)          |
| `isOpen`   | `boolean`  | `true` якщо склад закінчується голосною (відкритий склад)                 |

---

### `type: "whitespace"` — пробіл між словами

```json
{ "type": "whitespace" }
```

Вставляється між кожними двома сусідніми словами одного рядка. Жодних інших полів.

---

### `type: "line_break"` — межа між рядками

```json
{ "type": "line_break", "lineIndex": 0 }
```

| Поле        | Тип      | Опис                                            |
| ----------- | -------- | ----------------------------------------------- |
| `lineIndex` | `number` | 0-based індекс рядка, який **щойно закінчився** |

Вставляється між рядками. **Немає** trailing `line_break` після останнього рядка.

---

## Порядок елементів у потоці (гарантований)

```
[word] [whitespace] [word] [whitespace] [word]   ← рядок 0
[line_break lineIndex=0]
[word] [whitespace] [word]                        ← рядок 1
[line_break lineIndex=1]
[word]                                            ← рядок 2
```

Прості рядки на один вираз: `lineIndex` у `line_break` = `lineIndex` у попередніх `word`-елементах.

---

## Що НЕ потрапляє у потік

- Непідтверджені рядки
- `PUNCT` токени (коми, крапки, тире тощо)
- `GAP`, `HYPHEN`, `TAB`
- Слова без букв (напр. самотнє `"—"`)

---

## Повний приклад

Два підтверджені рядки: `"башук капуш"` / `"недоріка"`:

```json
{
  "metadata": {
    "version": "1.1",
    "generatedAt": "2026-05-08T14:22:00.000Z",
    "confirmedLineCount": 2,
    "totalWordCount": 3,
    "languagesPresent": ["uk"]
  },
  "stream": [
    {
      "type": "word",
      "id": "tok-001",
      "lineIndex": 0,
      "wordIndex": 0,
      "language": "uk",
      "original": "башук",
      "syllableCount": 2,
      "stressedSyllable": 1,
      "stressSource": "dict",
      "syllables": [
        {
          "ipa": "ba",
          "tokens": ["b", "a"],
          "grapheme": "ба",
          "stressed": false,
          "isOpen": true
        },
        {
          "ipa": "ʃuk",
          "tokens": ["ʃ", "u", "k"],
          "grapheme": "шук",
          "stressed": true,
          "isOpen": false
        }
      ]
    },
    { "type": "whitespace" },
    {
      "type": "word",
      "id": "tok-002",
      "lineIndex": 0,
      "wordIndex": 1,
      "language": "uk",
      "original": "капуш",
      "syllableCount": 2,
      "stressedSyllable": 1,
      "stressSource": "dict",
      "syllables": [
        {
          "ipa": "ka",
          "tokens": ["k", "a"],
          "grapheme": "ка",
          "stressed": false,
          "isOpen": true
        },
        {
          "ipa": "puʃ",
          "tokens": ["p", "u", "ʃ"],
          "grapheme": "пуш",
          "stressed": true,
          "isOpen": false
        }
      ]
    },
    { "type": "line_break", "lineIndex": 0 },
    {
      "type": "word",
      "id": "tok-003",
      "lineIndex": 1,
      "wordIndex": 0,
      "language": "uk",
      "original": "недоріка",
      "syllableCount": 4,
      "stressedSyllable": 2,
      "stressSource": "dict",
      "syllables": [
        {
          "ipa": "ne",
          "tokens": ["n", "e"],
          "grapheme": "не",
          "stressed": false,
          "isOpen": true
        },
        {
          "ipa": "do",
          "tokens": ["d", "o"],
          "grapheme": "до",
          "stressed": false,
          "isOpen": true
        },
        {
          "ipa": "rʲi",
          "tokens": ["rʲ", "i"],
          "grapheme": "рі",
          "stressed": true,
          "isOpen": true
        },
        {
          "ipa": "ka",
          "tokens": ["k", "a"],
          "grapheme": "ка",
          "stressed": false,
          "isOpen": true
        }
      ]
    }
  ]
}
```

---

## Round-trip: як бекенд повертає результат

Бекенд може відповісти будь-яким форматом, де ключ — `id` токена:

```json
{
  "annotations": {
    "tok-001": { "rhymeGroup": "A", "stressConfidence": 0.98 },
    "tok-002": { "rhymeGroup": "B", "stressConfidence": 0.95 },
    "tok-003": { "rhymeGroup": "A", "stressConfidence": 0.91 }
  }
}
```

Фронтенд знаходить токен по `id` у Pinia store і застосовує анотацію.
