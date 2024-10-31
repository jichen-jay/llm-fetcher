wasmtime serve --env TOGETHER_API_KEY=xxxx   -Scommon build/llm_fetcher_s.wasm

curl -X POST localhost:8080 --data '{ "text": "how much is 2+5" }'