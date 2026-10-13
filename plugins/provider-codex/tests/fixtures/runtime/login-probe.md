Codex CLI 0.160.0, macOS arm64, 2026-10-13. Disposable CODEX_HOME; stdio app-server.
Generated installed experimental TS bindings and live initialize/start/cancel probe.
No browser opened, credential submitted or completed login. OAuth query and one-time
user code redacted. The app-server owns callback listeners and its credential store.

Requests and observed results:

```json
{"method":"account/read","params":{"refreshToken":false}}
{"account":null,"requiresOpenaiAuth":true,"workspaceRouting":null}
{"method":"account/login/start","params":{"type":"chatgpt"}}
{"type":"chatgpt","loginId":"dd9ce65b-541e-44f0-bc79-700afe865859","authUrl":"https://auth.openai.com/oauth/authorize?<redacted>"}
{"method":"account/login/cancel","params":{"loginId":"dd9ce65b-541e-44f0-bc79-700afe865859"}}
{"status":"canceled"}
{"method":"account/login/completed","params":{"loginId":"dd9ce65b-541e-44f0-bc79-700afe865859","success":false,"error":"Login server error: Login was not completed","onboardingEntrypoint":null}}
{"method":"account/login/start","params":{"type":"chatgptDeviceCode"}}
{"type":"chatgptDeviceCode","loginId":"3bd79974-d5a3-4583-8d84-847d53dd3b75","verificationUrl":"https://auth.openai.com/codex/device","userCode":"<redacted>"}
{"method":"account/login/cancel","params":{"loginId":"3bd79974-d5a3-4583-8d84-847d53dd3b75"}}
{"status":"canceled"}
{"method":"account/login/completed","params":{"loginId":"3bd79974-d5a3-4583-8d84-847d53dd3b75","success":false,"error":"Login was not completed","onboardingEntrypoint":null}}
{"method":"account/login/start","params":{"type":"apiKey"}}
{"error":{"code":-32600,"message":"Invalid request: missing field `apiKey`"}}
```

Installed bindings declare API-key request `{type:"apiKey",apiKey:string}` and
response `{type:"apiKey"}`. Cancellation status is `canceled` or `notFound`.
Completion is `{loginId:string|null,success:boolean,error:string|null,onboardingEntrypoint:null|...}`.
Account update is `{authMode:null|"apiKey"|"chatgpt"|"chatgptAuthTokens"|...,planType:null|...}`.
The successful notification shape is schema evidence, not an executed login.
T3's reference was consulted for exclusivity, cancellation and retry ownership;
Domios delegates OAuth, callback-page and credential persistence to app-server.
