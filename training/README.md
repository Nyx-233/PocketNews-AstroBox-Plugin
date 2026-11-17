# Local news classification

The plugin combines two quantized linear SVMs with explicit geographic and main-event constraints. The scope model predicts `domestic`, `foreign`, or `unspecified`. The subject model predicts eleven subjects, including domestic governance and health. Clear foreign main events resolve to `world`; domestic subjects and unspecified locations resolve by subject. Chinese subjects in overseas civil litigation remain legal/social events. Publisher names, selected channels, and background product references are not geographic subjects.

The two models use independent title, 320-character lead, and 1,600-character body views with weights 1.0, 0.45, and 0.15. Features are Chinese character n-grams of lengths 1–4, complete ASCII word tokens, and soft domain/entity features. Numeric runs are normalized. Source attribution is removed before inference. Scope anchors and specific event constraints are applied in `classification.rs` after geography is established. They do not look up article IDs or reported headlines. Unknown or conflicting information uses a conservative margin gate. Margins are score differences, not calibrated probabilities.

## Training

Install NumPy, SciPy, scikit-learn, and joblib in the development environment. Obtain the public TNEWS archive and the seamew/THUCNews title distribution referenced in `THIRD_PARTY_NOTICES.md`. Convert the latter to JSONL records with `text` and integer `label` fields. Raw public datasets are not packaged.

```
py -3 scripts/train_classifier.py .tools/tnews --thuc .tools/classifier-v2/thuc-train.jsonl --evaluation-pool .tools/classifier-v2/evaluation-pool.jsonl
```

`classifier_features.py` defines feature extraction. `classifier_examples.py` defines 586 authored policy contrasts, separate from evaluation articles. Public labels are mapped and weakly refined for legal, medical, financial, and geographic evidence. This is supervised local training with weak labels and authored contrasts, not fine-tuning a pretrained language model. Model metadata contains input hashes, class lists, training counts, and output checksums. The binary format is `PNC2`: sorted 64-bit feature hashes, per-view TF-IDF values, signed 16-bit coefficients, and per-class biases. Runtime code is `text_model.rs`. No Python runtime or online inference is required on the phone.

## Evaluation

`evaluation.json` records the frozen end-to-end comparison against plugin 1.2.31. There are 55 development articles, 100 separate test articles, and five user-reported full-text regressions. The test split and reported labels were not used for model fitting, threshold selection, or decoder revisions. Development feedback was used for policy and feature changes. One development annotation was corrected from culture to entertainment after checking that the original event concerned a film.

`evaluation-manifest.json` identifies each development/test input by URL, group, split, label, and SHA-256. Original cached article bodies are intentionally excluded from the source distribution. The five reported articles were excluded from training along with their related headline families. No exact title overlaps were found between the 155 evaluation inputs and either public training corpus. Public TNEWS development titles were also excluded from training. Authored controls in `classifier-parity.json` check quantization and runtime parity; they are not accuracy samples.

The sample is small and source-specific, with uneven topic counts and no military test cases. It does not establish population-wide accuracy. Unknown foreign institutions, implicit locations, and multi-subject headlines remain error cases. Read both correct counts and acceptance coverage; a higher accepted precision with lower coverage alone does not establish a better classifier.

Use `cargo test --locked` for policy and numeric parity checks. The `classify` example reads JSONL from stdin and emits final categories, acceptance, and raw model diagnostics. The same example builds for `wasm32-wasip2`; it is excluded from the installable plugin's executable behavior.
