@echo off
rem Tai facebook/nllb-200-distilled-600M tu HuggingFace MOT LAN, doi sang CTranslate2 int8 vao models\nllb-600m-int8.
rem Can Python voi: pip install ctranslate2 transformers sentencepiece torch
rem Sau do chay: translator-ai model verify   (doi chieu SHA256 voi model-manifest.txt)
ct2-transformers-converter --model facebook/nllb-200-distilled-600M --output_dir models\nllb-600m-int8 --quantization int8 --copy_files tokenizer.json
