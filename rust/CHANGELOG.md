# Changelog

## [0.3.0](https://github.com/godaddy/cli/compare/v0.2.10...v0.3.0) (2026-08-24)


### ⚠ BREAKING CHANGES

* **env:** `--search` is now a real `search <query...> [--scope <path>]` command (from cli-engine 0.5.0), not a global flag that could attach to (and silently swallow the output of) any other command. Per-environment override env vars are now app-scoped (`GDDY_AUTH_URL`, `GDDY_TOKEN_URL`, `GDDY_DOMAINS_API_URL`, `GDDY_ACCOUNT_URL`) instead of environment-scoped (`<ENV>_OAUTH_AUTH_URL`, `<ENV>_OAUTH_TOKEN_URL`, `<ENV>_DOMAINS_API_URL`, `<ENV>_ACCOUNT_URL`); `api_url` is no longer env-var overridable at all. An environment defined only via `<PREFIX>_API_URL`, with no compiled-in or `environments.toml` entry, no longer resolves — every environment must now be a builtin (`ote`/`prod`) or a real `environments.toml` entry. A malformed (non-blank) override, from an env var or `environments.toml`, is now a hard error instead of silently falling back to the derived default (blank/ whitespace-only overrides still fall back, unchanged). An unknown or misconfigured `--env` is now a hard error everywhere, instead of silently falling back to prod's API URL.

### Features

* **actions-catalog:** add context-safe truncation for list/describe output ([#182](https://github.com/godaddy/cli/issues/182)) ([91cc165](https://github.com/godaddy/cli/commit/91cc165e6729548eb5ef7adfb16ec67a55575d98))
* add `domain operation status` to poll async domain operations ([#86](https://github.com/godaddy/cli/issues/86)) ([4ad277c](https://github.com/godaddy/cli/commit/4ad277c0b64cc2bae4707943c03b23adae195fe2))
* add gddy payments add command ([f28deb2](https://github.com/godaddy/cli/commit/f28deb21ec089a7ef19e6b0a0c2d33ebb5ea866a))
* add gddy payments add command ([42e3be6](https://github.com/godaddy/cli/commit/42e3be6e86465e8c1d0948c9f5808ed799bc099c))
* add more next-actions across commands ([#83](https://github.com/godaddy/cli/issues/83)) ([a62460b](https://github.com/godaddy/cli/commit/a62460bf3091a7db695da63848295eb9b8b1ad1c))
* add Personal Access Token (PAT) support ([#80](https://github.com/godaddy/cli/issues/80)) ([827d448](https://github.com/godaddy/cli/commit/827d4485b2e7c1f90d2b546962c0d32d21d0bfe3))
* **api-catalog:** add drift guards for the embedded catalog (DEVX-549) ([#98](https://github.com/godaddy/cli/issues/98)) ([3605d8e](https://github.com/godaddy/cli/commit/3605d8e96a8c4c895da1821e1258c8f01bec8254))
* **api-explorer:** add `api graphql` command group for GraphQL-backed domains ([#200](https://github.com/godaddy/cli/issues/200)) ([c35ecd8](https://github.com/godaddy/cli/commit/c35ecd8a6348b0b6dacb543ed4f8746e36757a82))
* **api-explorer:** add command hyperlinks for large output [DEVEX-967] ([#187](https://github.com/godaddy/cli/issues/187)) ([bfcc4be](https://github.com/godaddy/cli/commit/bfcc4be60083c11b0c797ff95024bf489170766c))
* **api-explorer:** support `api call` by operation id with parameter substitution ([#209](https://github.com/godaddy/cli/issues/209)) ([cd8890a](https://github.com/godaddy/cli/commit/cd8890a91b2a22e2f48c47d53568b20f684bc22d))
* **api:** restore agent-safe describe/list/search output [DEVEX-702] ([#169](https://github.com/godaddy/cli/issues/169)) ([1f33ec6](https://github.com/godaddy/cli/commit/1f33ec604cd9837fcd1d8ab4f16f8d3d19204135))
* **app:** add settings support for app registration ([#216](https://github.com/godaddy/cli/issues/216)) ([74934c2](https://github.com/godaddy/cli/commit/74934c266d580a247b146767fd3802e0bd240eb0))
* **application init:** gate creation on onboarding (DEVX-545) ([#139](https://github.com/godaddy/cli/issues/139)) ([afa87e2](https://github.com/godaddy/cli/commit/afa87e235ad995d65907dbc8ac9673c708e621fd))
* **auth:** add live OAuth scope registry via `gddy auth scopes` (DEVEX-886, DEVEX-891) ([#116](https://github.com/godaddy/cli/issues/116)) ([cb16cc9](https://github.com/godaddy/cli/commit/cb16cc9dfa1fcc717e71b70a5c94e41e0c787bcf))
* **cli:** convert command args to cli-engine's typed-args API ([#181](https://github.com/godaddy/cli/issues/181)) ([b6bf793](https://github.com/godaddy/cli/commit/b6bf79300d04df023503628e1dd7c0785cd515c9))
* **cli:** opt into cli-engine's pagination for domain/dns list and api search [DEVEX-972] ([#189](https://github.com/godaddy/cli/issues/189)) ([8482450](https://github.com/godaddy/cli/commit/8482450805073acc1050ca800ca7f6ba0f2ae3a4))
* **cli:** right-align numeric table columns on cli-engine 0.8.6 ([#219](https://github.com/godaddy/cli/issues/219)) ([2647579](https://github.com/godaddy/cli/commit/2647579ee113183099cf0cf243d0bcb37aa428d3))
* default to prod, fix cross-platform config writes, support Windows installs ([#54](https://github.com/godaddy/cli/issues/54)) ([43ad0ad](https://github.com/godaddy/cli/commit/43ad0ade523d26ff61851047ff5fc9128c441bf0))
* DEV/TEST environments + OAuth scope step-up (DEVEX-719) ([#57](https://github.com/godaddy/cli/issues/57)) ([32cd77e](https://github.com/godaddy/cli/commit/32cd77e3ed66e3e0f183f9b8e627c2e18fb090be))
* DNS record management + domain list, with output-field discovery ([#65](https://github.com/godaddy/cli/issues/65)) ([fa66cfa](https://github.com/godaddy/cli/commit/fa66cfa34b20eff2687f1f6e84510bcc6c1dfa51))
* **dns:** add --replace-conflicting-types flag, fix CNAME conflict errors ([#102](https://github.com/godaddy/cli/issues/102)) ([2c0e576](https://github.com/godaddy/cli/commit/2c0e576048e393af8d35c78ed2bee6cbfa78af4f))
* domain availability + suggest via spec-generated client ([#59](https://github.com/godaddy/cli/issues/59)) ([f56cd21](https://github.com/godaddy/cli/commit/f56cd21b47856ce2792a2000a6209cfc6ed7e549))
* **domain list:** hide cancelled/non-visible domains by default ([#94](https://github.com/godaddy/cli/issues/94)) ([ad17cd3](https://github.com/godaddy/cli/commit/ad17cd33f1f0c54723b97ee718a5fc48b856211a))
* domain purchase (v2 register) + domain get ([#70](https://github.com/godaddy/cli/issues/70)) ([96696ca](https://github.com/godaddy/cli/commit/96696ca8c99d506261f727954540dd28c414649f))
* **domain:** show registration period with its unit in the default table ([#199](https://github.com/godaddy/cli/issues/199)) ([4de0e3e](https://github.com/godaddy/cli/commit/4de0e3e32716ea5b75603b1784d7578ab9cb0aa9))
* **domain:** support premium domains and full-term pricing in list/available ([8c2367a](https://github.com/godaddy/cli/commit/8c2367a8431df75e47532ca934cb9faae9028a2c))
* **dry-run:** let dry-run validate input and preview real effects ([#141](https://github.com/godaddy/cli/issues/141)) ([6651c6b](https://github.com/godaddy/cli/commit/6651c6be293aa93fc48503a7486353d23d2aadc1))
* **email:** Added CLI support for panel based email services ([#212](https://github.com/godaddy/cli/issues/212)) ([c99004a](https://github.com/godaddy/cli/commit/c99004a571517b045d67c05540c5936d6fcc666e))
* **env:** consolidate onto cli-engine's shared Environments, wire feature-flag stages (DEVEX-929) ([#145](https://github.com/godaddy/cli/issues/145)) ([a6f48fe](https://github.com/godaddy/cli/commit/a6f48fe3dbbe4996e0ba6b540cc284637ffd698a))
* **environments:** read DevX Core URL from config (DEVX-793) ([#190](https://github.com/godaddy/cli/issues/190)) ([d772045](https://github.com/godaddy/cli/commit/d7720453d67240df97fa1b945d390f7ad2a80d0b))
* **env:** migrate gddy onto cli-engine's declarative EnvConfig (DEVEX-947) ([#171](https://github.com/godaddy/cli/issues/171)) ([44f7368](https://github.com/godaddy/cli/commit/44f73684d92548c054b44c45dc8028c79215ff25))
* **errors:** map failures to stable codes and top-level fix [DEVEX-945] ([#162](https://github.com/godaddy/cli/issues/162)) ([576641c](https://github.com/godaddy/cli/commit/576641c9416cd2ec2cf85cb1d39d1bfd32ffb93f))
* flatten domain suggest term pricing and prefix next-actions with gddy ([#90](https://github.com/godaddy/cli/issues/90)) ([6f7a7a9](https://github.com/godaddy/cli/commit/6f7a7a9f2a13536a901f827c21e9804386f18812))
* Improve guides ([#210](https://github.com/godaddy/cli/issues/210)) ([3832ff0](https://github.com/godaddy/cli/commit/3832ff0e93ce881a7979d8c4cc59322c329a1512))
* integrate Node.js Hosting public API with new CLI ([#78](https://github.com/godaddy/cli/issues/78)) ([8877abe](https://github.com/godaddy/cli/commit/8877abe53be39da35e3e4c8a91390097530eb5a0))
* migrate domain & dns commands to the v3 Domains API ([#76](https://github.com/godaddy/cli/issues/76)) ([c39fbeb](https://github.com/godaddy/cli/commit/c39fbebdacd51033e9cf6e0b94c75c9b95be74c3))
* **output:** render nested arrays/objects as tables instead of raw JSON [DEVEX-968] ([#180](https://github.com/godaddy/cli/issues/180)) ([9c15fd2](https://github.com/godaddy/cli/commit/9c15fd295a51cbff52d6838dff624147a6f1f172))
* **paas:** add GitHub operations for Node.js Hosting CLI command group ([#135](https://github.com/godaddy/cli/issues/135)) ([1de4b88](https://github.com/godaddy/cli/commit/1de4b881e281626a3b3dd764d1452efbf7f5e9d7))
* **platform:** namespace Developer Platform commands (DEVX-621) ([#149](https://github.com/godaddy/cli/issues/149)) ([faf89cc](https://github.com/godaddy/cli/commit/faf89cc8ee0ccbd82bfefda9535a569ee629ace0))
* **platform:** promote namespace to GA ([#223](https://github.com/godaddy/cli/issues/223)) ([8b9dd21](https://github.com/godaddy/cli/commit/8b9dd219b4f4dac286d865f239777f2ca1cb3983))
* publish gddy alpha binary on rust-port pushes; rename binary to gddy ([#53](https://github.com/godaddy/cli/issues/53)) ([9a37bde](https://github.com/godaddy/cli/commit/9a37bdeaebea61f87a6c08c4909fefbabcd9e180))
* **release:** release-please, self-update, no-sudo installs ([#88](https://github.com/godaddy/cli/issues/88)) ([e66f990](https://github.com/godaddy/cli/commit/e66f990f4fb504df743559058ade23d4d210e32f))
* reorganize module categories and set feature-flag stages ([#87](https://github.com/godaddy/cli/issues/87)) ([7ddf2c4](https://github.com/godaddy/cli/commit/7ddf2c41f196be6a901728ca61394e61593d44ea))
* **update:** nudge shell-completion refresh after a real update ([#197](https://github.com/godaddy/cli/issues/197)) ([7b49a46](https://github.com/godaddy/cli/commit/7b49a4601e6d8385340db2c6678480b0c88c2675))
* **webhook:** Normalize webhook output and truncate larger results ([#165](https://github.com/godaddy/cli/issues/165)) ([af59d11](https://github.com/godaddy/cli/commit/af59d11afc04bed03c64ef2dbdf80678a115ca4e))


### Bug Fixes

* address PR review — account_url in environments module, URL on browser failure ([e5966ed](https://github.com/godaddy/cli/commit/e5966ed70dfe4c04902be83647f4a464ef60d305))
* adopt cli-engine 0.2.0 fail-closed auth; mark local commands no_auth ([#56](https://github.com/godaddy/cli/issues/56)) ([1cc8d98](https://github.com/godaddy/cli/commit/1cc8d9878e821455c900f33d50b174953621c5b7))
* **api-call:** apply headers, error on non-2xx, GraphQL errors, output shape (DEVX-546) ([#97](https://github.com/godaddy/cli/issues/97)) ([29529c6](https://github.com/godaddy/cli/commit/29529c61dcdcbced7a6214616f19b31153acc2c5))
* **api-catalog:** distinguish $defs keys for same-file property refs [DEVEX-965] ([#168](https://github.com/godaddy/cli/issues/168)) ([80d3abe](https://github.com/godaddy/cli/commit/80d3abebd25192df226f068f2e0fa6854df6ebd4))
* **api-catalog:** parse federated GraphQL schemas and stabilize manifest order ([#204](https://github.com/godaddy/cli/issues/204)) ([118f3f2](https://github.com/godaddy/cli/commit/118f3f2bc44cdc3022b5f85c93c747a81eef6131))
* **api-catalog:** resolve discriminator.mapping refs to #/$defs pointers (DEVX-548) ([#96](https://github.com/godaddy/cli/issues/96)) ([0b9d565](https://github.com/godaddy/cli/commit/0b9d56542b84e3e93bb0497f5c99291e6237f0eb))
* **api-catalog:** resync drift, normalize domains base URL, sort + env-aware routing ([#167](https://github.com/godaddy/cli/issues/167)) ([339db2d](https://github.com/godaddy/cli/commit/339db2dab8bea36d8e411dcc5699f184295e857a))
* **api-explorer:** remove broken --query flag, add domains catalog source (DEVEX-898) ([#124](https://github.com/godaddy/cli/issues/124)) ([c07994a](https://github.com/godaddy/cli/commit/c07994a9fa7c141b10db610cca20eea383b7565e))
* **application deploy:** activate release and promote app to ACTIVE (DEVEX-704) ([#103](https://github.com/godaddy/cli/issues/103)) ([87457ef](https://github.com/godaddy/cli/commit/87457ef6545a51db005205972f2436068604284d))
* **application deploy:** guarantee a terminal result/error NDJSON event (DEVX-544) ([#114](https://github.com/godaddy/cli/issues/114)) ([1d0fb2d](https://github.com/godaddy/cli/commit/1d0fb2d3d04b9ef5201d0322b2104e6e638f4573))
* **application init:** align with TS config seeding and validation ([#126](https://github.com/godaddy/cli/issues/126)) (DEVEX-707) ([1cfcda2](https://github.com/godaddy/cli/commit/1cfcda210edb305615a92ae433e11833392bcd85))
* **application update:** restore --status `ACTIVE | INACTIVE` ([#132](https://github.com/godaddy/cli/issues/132)) [DEVEX-709] ([565e0d7](https://github.com/godaddy/cli/commit/565e0d75d956d18823338605eac0198964167128))
* **application validate:** restore remote application state checks ([#134](https://github.com/godaddy/cli/issues/134)) [DEVEX-708] ([b84ddb0](https://github.com/godaddy/cli/commit/b84ddb05c42c1b66e571818e3759f20e108347a0))
* **auth:** report per-environment status and refresh-token availability ([#184](https://github.com/godaddy/cli/issues/184)) ([cf7d9f5](https://github.com/godaddy/cli/commit/cf7d9f554c9812f19cab14576d46f081cbfbba50))
* **auth:** validate requested OAuth scopes against the CLI's registry (DEVEX-894) ([#108](https://github.com/godaddy/cli/issues/108)) ([6abd2ee](https://github.com/godaddy/cli/commit/6abd2ee7b52be482f68a137a8527dc469da261a0))
* **cli:** drop experimental platform suggestion, fix stale --search doc ([#174](https://github.com/godaddy/cli/issues/174)) ([48047d1](https://github.com/godaddy/cli/commit/48047d11c398833da755d35e5d6e7fcb799fa908))
* **cli:** render platform actions list as a table with pagination ([#198](https://github.com/godaddy/cli/issues/198)) ([3c7deca](https://github.com/godaddy/cli/commit/3c7deca563946ffa3563e56fbd93895ca079574f))
* **config:** restore godaddy.toml schema validation [DEVEX-714] ([#166](https://github.com/godaddy/cli/issues/166)) ([4cfa3e4](https://github.com/godaddy/cli/commit/4cfa3e4afd22684116e117f4313f58970514115c))
* **dns:** stop `dns set` from wiping zones via the v3 replace-record PUT ([#143](https://github.com/godaddy/cli/issues/143)) ([90c2408](https://github.com/godaddy/cli/commit/90c240831e20104a7dcc326fc3b7b0a57469af0e))
* **domain suggest:** validate --limit against the v3 API's 50-suggestion cap (DEVEX-883) ([#117](https://github.com/godaddy/cli/issues/117)) ([c409719](https://github.com/godaddy/cli/commit/c409719be3344ede4eb3ffe1e487c13e4f023f1a))
* domain-purchase data formatting + comprehensive CLI help ([#75](https://github.com/godaddy/cli/issues/75)) ([5d15d6d](https://github.com/godaddy/cli/commit/5d15d6d4b499c34c68ba21854ba719af293355ee))
* **domain:** comma-join repeatable TLD flags before sending as query params (DEVEX-882) ([#106](https://github.com/godaddy/cli/issues/106)) ([ada4a93](https://github.com/godaddy/cli/commit/ada4a93700f9a9d80e5bf8b8b1adb3dcc8aa6ac3))
* **domain:** show renewal price consistently across available/quote/suggest (GDDEVPLAT-133) ([#125](https://github.com/godaddy/cli/issues/125)) ([71e4eea](https://github.com/godaddy/cli/commit/71e4eea5da9a6d8a547560e40b211229f9213166))
* **domain:** validate domain and nameserver hostname shape before the API call ([#127](https://github.com/godaddy/cli/issues/127)) ([9348495](https://github.com/godaddy/cli/commit/934849587684cc4f221b94bae1f162c95aa69a66))
* **env:** add app config summary to env info [DEVEX-715] ([#185](https://github.com/godaddy/cli/issues/185)) ([9f85943](https://github.com/godaddy/cli/commit/9f85943e3f97165bc62d378b9e3ff720de390002))
* **extensions:** restore pre-bundle AST security scan [DEVEX-710] ([#208](https://github.com/godaddy/cli/issues/208)) ([e3a2010](https://github.com/godaddy/cli/commit/e3a2010f6040bc9c6781f52a72d1dbab7a5a1e88))
* **extensions:** restore UI extension targets on add + deploy (DEVX-541) ([#107](https://github.com/godaddy/cli/issues/107)) ([247f712](https://github.com/godaddy/cli/commit/247f71254c1916b0126384e81af325863be84957))
* **extensions:** Rust bundler/deploy path parity with TS [DEVEX-712] ([#177](https://github.com/godaddy/cli/issues/177)) ([e995f99](https://github.com/godaddy/cli/commit/e995f990f00d5d1e4ec9afc44ac1631691d0a977))
* Mapped response to { eventType, description } per event, added 50-item truncation with temp file fallback, and returned { events, total, shown, truncated, full_output } matching the original TS behavior ([af59d11](https://github.com/godaddy/cli/commit/af59d11afc04bed03c64ef2dbdf80678a115ca4e))
* Node.js Hosting CLI deployments --limit flag typing issue ([#183](https://github.com/godaddy/cli/issues/183)) ([4028ff1](https://github.com/godaddy/cli/commit/4028ff1f612ddbf52d894e9c0ee0096af429ab7f))
* omit null settings presentation fields ([#225](https://github.com/godaddy/cli/issues/225)) ([ef745d9](https://github.com/godaddy/cli/commit/ef745d98f5be9bb2dda423648a73b6e64048bc25))
* **onboarding:** send CLI User-Agent header ([#157](https://github.com/godaddy/cli/issues/157)) ([467c7f7](https://github.com/godaddy/cli/commit/467c7f74bd35948fddbfa6c02529ba6f85d4cbed))
* **pat:** stop over-specifying PAT shape in error and docs (DEVEX-889) ([#131](https://github.com/godaddy/cli/issues/131)) ([92144a3](https://github.com/godaddy/cli/commit/92144a3f3a1ac041b86ac26eb1a7eb06999d503d))
* **payment-methods:** rename gddy payments to gddy payment-methods (DEVEX-900) ([#119](https://github.com/godaddy/cli/issues/119)) ([67512bc](https://github.com/godaddy/cli/commit/67512bc39b91a4a45e360ac127b7165c82124439))
* payments add always returns URL, treats browser failure as non-fatal ([#72](https://github.com/godaddy/cli/issues/72)) ([07cba90](https://github.com/godaddy/cli/commit/07cba903693f322e22b9f1fc228ebc5f0ae5a404))
* **platform:** complete deploy stream and sync manifest metadata ([#217](https://github.com/godaddy/cli/issues/217)) ([f3dfb2e](https://github.com/godaddy/cli/commit/f3dfb2ef867b6f80ec7899a7747608fd4edebb89))
* **platform:** list apps across all App Registry statuses ([#206](https://github.com/godaddy/cli/issues/206)) ([737e91b](https://github.com/godaddy/cli/commit/737e91b60a1375385d11cb772d521ba3e4175c8d))
* **platform:** remove direct application status updates (DEVX-927) ([#218](https://github.com/godaddy/cli/issues/218)) ([ab07767](https://github.com/godaddy/cli/commit/ab07767c30115f13f724d7b404fb96b922e6f6ef))
* **release:** include actions, subscriptions, UI extensions in release (DEVX-540) ([#100](https://github.com/godaddy/cli/issues/100)) ([afd8d9b](https://github.com/godaddy/cli/commit/afd8d9beda13a130bc959cdbfc1925b251ec9e1b))
* **release:** update version argument support ([#195](https://github.com/godaddy/cli/issues/195)) ([2964db2](https://github.com/godaddy/cli/commit/2964db2fd567e5d389270f323b62c5afb810d0fe))
* request offline_access scope for refresh tokens, centralize hosting scopes ([#85](https://github.com/godaddy/cli/issues/85)) ([b6d49c7](https://github.com/godaddy/cli/commit/b6d49c7c7e0a29e8c3b9808332efe597008367ca))
* route all HTTP clients through --debug transport logger ([#81](https://github.com/godaddy/cli/issues/81)) ([d0c94a3](https://github.com/godaddy/cli/commit/d0c94a305b1792073d3a7760ce4a6ac3616ddc11))
* stop truncating agreement URLs in `domain agreements` human output ([#82](https://github.com/godaddy/cli/issues/82)) ([6cf9112](https://github.com/godaddy/cli/commit/6cf911265f063ce164aab92e16e619fb9f379ca5))
* surface failure detail on domain purchase, unify cli-engine version ([#84](https://github.com/godaddy/cli/issues/84)) ([d3a9cd1](https://github.com/godaddy/cli/commit/d3a9cd1930167f2f947987c292ed280205fae82d))
* **tests:** Close Rust test-coverage gaps ([#214](https://github.com/godaddy/cli/issues/214)) ([e26912b](https://github.com/godaddy/cli/commit/e26912bb027aa68de387815646689bdf8c60e80e))
* use non-API GitHub redirect for update checks, add --force to update apply ([#91](https://github.com/godaddy/cli/issues/91)) ([60cc92d](https://github.com/godaddy/cli/commit/60cc92d7a325b7d65350b2599e494b716bc1d222))


### Documentation

* Improve new user experience ([#202](https://github.com/godaddy/cli/issues/202)) ([90fb564](https://github.com/godaddy/cli/commit/90fb564e4d04b307b78866b44605e0b7ea394f15))


### Miscellaneous

* bump cli-engine to 0.3.4 for non-interactive scope step-up ([#73](https://github.com/godaddy/cli/issues/73)) ([206c2b3](https://github.com/godaddy/cli/commit/206c2b3277478e69d46019681e0bb53e38578943))
* **cicd:** Add audit step and regenerate lock file ([#158](https://github.com/godaddy/cli/issues/158)) ([4ed50e8](https://github.com/godaddy/cli/commit/4ed50e8ba0c747744bccbdb78ca0f903759345cf))
* **main:** release 0.1.15 ([#160](https://github.com/godaddy/cli/issues/160)) ([7fc16d5](https://github.com/godaddy/cli/commit/7fc16d599ea4a22382ac666a96376314ec4c8207))
* **main:** release 0.1.16 ([#163](https://github.com/godaddy/cli/issues/163)) ([484bd39](https://github.com/godaddy/cli/commit/484bd398d370c972adad0f64136d4c1c092b0fbc))
* **main:** release 0.2.0 ([#170](https://github.com/godaddy/cli/issues/170)) ([e2dd40a](https://github.com/godaddy/cli/commit/e2dd40a238955c1395a29865c61e693fc2fda53d))
* **main:** release 0.2.1 ([#175](https://github.com/godaddy/cli/issues/175)) ([8899589](https://github.com/godaddy/cli/commit/8899589ef9b32b19d8810c56d83273736ad1f615))
* **main:** release 0.2.10 ([#226](https://github.com/godaddy/cli/issues/226)) ([44df6c9](https://github.com/godaddy/cli/commit/44df6c92f3340f9401565fa26324b1b59f8c3746))
* **main:** release 0.2.2 ([#179](https://github.com/godaddy/cli/issues/179)) ([403a4cf](https://github.com/godaddy/cli/commit/403a4cf67934e59e6943e6aa8856124792ba1098))
* **main:** release 0.2.3 ([#194](https://github.com/godaddy/cli/issues/194)) ([7ca98ea](https://github.com/godaddy/cli/commit/7ca98eacb557ac191f8315546db40ba31dde9f6e))
* **main:** release 0.2.4 ([#196](https://github.com/godaddy/cli/issues/196)) ([d875379](https://github.com/godaddy/cli/commit/d875379c8471945d27c2a74f7b32448703997094))
* **main:** release 0.2.5 ([#203](https://github.com/godaddy/cli/issues/203)) ([a03bc02](https://github.com/godaddy/cli/commit/a03bc023cc41992a4c4935f3846f7557e0ad7e44))
* **main:** release 0.2.6 ([#207](https://github.com/godaddy/cli/issues/207)) ([4f0234c](https://github.com/godaddy/cli/commit/4f0234cb571a0680f5d7f015b4c7aae9156358db))
* **main:** release 0.2.7 ([#215](https://github.com/godaddy/cli/issues/215)) ([5ee9826](https://github.com/godaddy/cli/commit/5ee982647d758368a26e7b1db9b42a922aa80566))
* **main:** release 0.2.8 ([#220](https://github.com/godaddy/cli/issues/220)) ([8aba79c](https://github.com/godaddy/cli/commit/8aba79cb31112329be1b9ab4b37407b37bdfed3c))
* **main:** release 0.2.9 ([#221](https://github.com/godaddy/cli/issues/221)) ([6e43cc7](https://github.com/godaddy/cli/commit/6e43cc7ced908501a18eaff844d784a684479fc0))
* **nodejs:** update --help descriptions for Node.js Hosting CLI com… ([#186](https://github.com/godaddy/cli/issues/186)) ([71ae7a5](https://github.com/godaddy/cli/commit/71ae7a5a55a863544a24725a927d45ac01ed8893))
* **rust-port:** release 0.1.1 ([#89](https://github.com/godaddy/cli/issues/89)) ([b5cbbc0](https://github.com/godaddy/cli/commit/b5cbbc02b7d2ff0f4d979fd964c5214623bd6a8c))
* **rust-port:** release 0.1.10 ([#128](https://github.com/godaddy/cli/issues/128)) ([a2fc6d8](https://github.com/godaddy/cli/commit/a2fc6d8ba2def7dea08286c97d6ba368ab1d157a))
* **rust-port:** release 0.1.11 ([#130](https://github.com/godaddy/cli/issues/130)) ([5468cf8](https://github.com/godaddy/cli/commit/5468cf8257d51ead60e843755f3169bd22e57918))
* **rust-port:** release 0.1.12 ([#133](https://github.com/godaddy/cli/issues/133)) ([f21c11c](https://github.com/godaddy/cli/commit/f21c11c686994bbea672ce9c2bad666405fff91c))
* **rust-port:** release 0.1.13 ([#138](https://github.com/godaddy/cli/issues/138)) ([b025d55](https://github.com/godaddy/cli/commit/b025d559b50edec8b51f956928785792f9e03240))
* **rust-port:** release 0.1.14 ([#146](https://github.com/godaddy/cli/issues/146)) ([4bb0206](https://github.com/godaddy/cli/commit/4bb0206e61d05b3084bbdbebf6bdb6730c23ea3c))
* **rust-port:** release 0.1.2 ([#92](https://github.com/godaddy/cli/issues/92)) ([e81a9e4](https://github.com/godaddy/cli/commit/e81a9e47afca18dca1735083982326243cb495df))
* **rust-port:** release 0.1.3 ([#95](https://github.com/godaddy/cli/issues/95)) ([a3da71b](https://github.com/godaddy/cli/commit/a3da71b962b6c8d26444cdfde9d88ec4d629c407))
* **rust-port:** release 0.1.4 ([#105](https://github.com/godaddy/cli/issues/105)) ([6653a5c](https://github.com/godaddy/cli/commit/6653a5cecf10c167de4d2e334356e2d8e548237b))
* **rust-port:** release 0.1.5 ([#112](https://github.com/godaddy/cli/issues/112)) ([f817fe0](https://github.com/godaddy/cli/commit/f817fe0e05b90ad27e12b694694c697837fe24d2))
* **rust-port:** release 0.1.6 ([#120](https://github.com/godaddy/cli/issues/120)) ([0bcf9ee](https://github.com/godaddy/cli/commit/0bcf9ee5ed81f2d5996df9636d8e822cc004970d))
* **rust-port:** release 0.1.7 ([#121](https://github.com/godaddy/cli/issues/121)) ([759415e](https://github.com/godaddy/cli/commit/759415e086445b16ff59f2515d1b25e224dbcea8))
* **rust-port:** release 0.1.8 ([#122](https://github.com/godaddy/cli/issues/122)) ([01919c7](https://github.com/godaddy/cli/commit/01919c71c33fadf9b9f885851a43250db4d9b01a))
* **rust-port:** release 0.1.9 ([#123](https://github.com/godaddy/cli/issues/123)) ([2c2c2cc](https://github.com/godaddy/cli/commit/2c2c2ccfe11d38d42b9d1b8863c898e575256818))
* **rust:** enforce command-tree file structure; split oversized modules ([#192](https://github.com/godaddy/cli/issues/192)) ([0b5b131](https://github.com/godaddy/cli/commit/0b5b1312c65c8e9607754b8672ee1b3f6d60c594))
* **scanner:** Review Scan blocking rules ([acc3c9c](https://github.com/godaddy/cli/commit/acc3c9caeb4742eb518400c6cfff6291067cbe04))
* **scanner:** Review Scan blocking rules ([#191](https://github.com/godaddy/cli/issues/191)) ([acc3c9c](https://github.com/godaddy/cli/commit/acc3c9caeb4742eb518400c6cfff6291067cbe04))


### Code Refactoring

* **environments:** split mod.rs into config/devx_core/catalog submodules ([#213](https://github.com/godaddy/cli/issues/213)) ([4bf3a00](https://github.com/godaddy/cli/commit/4bf3a009bb8cfb9cc305586a45e2e6586c49cfbc))
* improve agent UX for Node.js Hosting CLI commands ([#153](https://github.com/godaddy/cli/issues/153)) ([ff56ec6](https://github.com/godaddy/cli/commit/ff56ec6c90dbd9242d366202eafaa887794faa77))


### Tests

* **api:** cover commerce scope step-up inputs ([#150](https://github.com/godaddy/cli/issues/150)) ([403dc71](https://github.com/godaddy/cli/commit/403dc71674d1edb939c8dcf624ebbe352dc38ed8))

## [0.2.10](https://github.com/godaddy/cli/compare/v0.2.9...v0.2.10) (2026-08-24)


### Features

* **email:** Added CLI support for panel based email services ([#212](https://github.com/godaddy/cli/issues/212)) ([c99004a](https://github.com/godaddy/cli/commit/c99004a571517b045d67c05540c5936d6fcc666e))


### Bug Fixes

* **extensions:** restore pre-bundle AST security scan [DEVEX-710] ([#208](https://github.com/godaddy/cli/issues/208)) ([e3a2010](https://github.com/godaddy/cli/commit/e3a2010f6040bc9c6781f52a72d1dbab7a5a1e88))
* omit null settings presentation fields ([#225](https://github.com/godaddy/cli/issues/225)) ([ef745d9](https://github.com/godaddy/cli/commit/ef745d98f5be9bb2dda423648a73b6e64048bc25))

## [0.2.9](https://github.com/godaddy/cli/compare/v0.2.8...v0.2.9) (2026-08-21)


### Features

* **app:** add settings support for app registration ([#216](https://github.com/godaddy/cli/issues/216)) ([74934c2](https://github.com/godaddy/cli/commit/74934c266d580a247b146767fd3802e0bd240eb0))
* **platform:** promote namespace to GA ([#223](https://github.com/godaddy/cli/issues/223)) ([8b9dd21](https://github.com/godaddy/cli/commit/8b9dd219b4f4dac286d865f239777f2ca1cb3983))

## [0.2.8](https://github.com/godaddy/cli/compare/v0.2.7...v0.2.8) (2026-08-19)


### Features

* **cli:** right-align numeric table columns on cli-engine 0.8.6 ([#219](https://github.com/godaddy/cli/issues/219)) ([2647579](https://github.com/godaddy/cli/commit/2647579ee113183099cf0cf243d0bcb37aa428d3))

## [0.2.7](https://github.com/godaddy/cli/compare/v0.2.6...v0.2.7) (2026-08-18)


### Features

* **domain:** support premium domains and full-term pricing in list/available ([8c2367a](https://github.com/godaddy/cli/commit/8c2367a8431df75e47532ca934cb9faae9028a2c))
* **environments:** read DevX Core URL from config (DEVX-793) ([#190](https://github.com/godaddy/cli/issues/190)) ([d772045](https://github.com/godaddy/cli/commit/d7720453d67240df97fa1b945d390f7ad2a80d0b))


### Bug Fixes

* **platform:** complete deploy stream and sync manifest metadata ([#217](https://github.com/godaddy/cli/issues/217)) ([f3dfb2e](https://github.com/godaddy/cli/commit/f3dfb2ef867b6f80ec7899a7747608fd4edebb89))
* **platform:** remove direct application status updates (DEVX-927) ([#218](https://github.com/godaddy/cli/issues/218)) ([ab07767](https://github.com/godaddy/cli/commit/ab07767c30115f13f724d7b404fb96b922e6f6ef))
* **tests:** Close Rust test-coverage gaps ([#214](https://github.com/godaddy/cli/issues/214)) ([e26912b](https://github.com/godaddy/cli/commit/e26912bb027aa68de387815646689bdf8c60e80e))


### Code Refactoring

* **environments:** split mod.rs into config/devx_core/catalog submodules ([#213](https://github.com/godaddy/cli/issues/213)) ([4bf3a00](https://github.com/godaddy/cli/commit/4bf3a009bb8cfb9cc305586a45e2e6586c49cfbc))

## [0.2.6](https://github.com/godaddy/cli/compare/v0.2.5...v0.2.6) (2026-08-17)


### Features

* **actions-catalog:** add context-safe truncation for list/describe output ([#182](https://github.com/godaddy/cli/issues/182)) ([91cc165](https://github.com/godaddy/cli/commit/91cc165e6729548eb5ef7adfb16ec67a55575d98))
* **api-explorer:** support `api call` by operation id with parameter substitution ([#209](https://github.com/godaddy/cli/issues/209)) ([cd8890a](https://github.com/godaddy/cli/commit/cd8890a91b2a22e2f48c47d53568b20f684bc22d))
* Improve guides ([#210](https://github.com/godaddy/cli/issues/210)) ([3832ff0](https://github.com/godaddy/cli/commit/3832ff0e93ce881a7979d8c4cc59322c329a1512))

## [0.2.5](https://github.com/godaddy/cli/compare/v0.2.4...v0.2.5) (2026-08-12)


### Features

* **api-explorer:** add `api graphql` command group for GraphQL-backed domains ([#200](https://github.com/godaddy/cli/issues/200)) ([c35ecd8](https://github.com/godaddy/cli/commit/c35ecd8a6348b0b6dacb543ed4f8746e36757a82))


### Bug Fixes

* **api-catalog:** parse federated GraphQL schemas and stabilize manifest order ([#204](https://github.com/godaddy/cli/issues/204)) ([118f3f2](https://github.com/godaddy/cli/commit/118f3f2bc44cdc3022b5f85c93c747a81eef6131))
* **platform:** list apps across all App Registry statuses ([#206](https://github.com/godaddy/cli/issues/206)) ([737e91b](https://github.com/godaddy/cli/commit/737e91b60a1375385d11cb772d521ba3e4175c8d))


### Documentation

* Improve new user experience ([#202](https://github.com/godaddy/cli/issues/202)) ([90fb564](https://github.com/godaddy/cli/commit/90fb564e4d04b307b78866b44605e0b7ea394f15))


### Miscellaneous

* **scanner:** Review Scan blocking rules ([acc3c9c](https://github.com/godaddy/cli/commit/acc3c9caeb4742eb518400c6cfff6291067cbe04))
* **scanner:** Review Scan blocking rules ([#191](https://github.com/godaddy/cli/issues/191)) ([acc3c9c](https://github.com/godaddy/cli/commit/acc3c9caeb4742eb518400c6cfff6291067cbe04))

## [0.2.4](https://github.com/godaddy/cli/compare/v0.2.3...v0.2.4) (2026-08-11)


### Features

* **domain:** show registration period with its unit in the default table ([#199](https://github.com/godaddy/cli/issues/199)) ([4de0e3e](https://github.com/godaddy/cli/commit/4de0e3e32716ea5b75603b1784d7578ab9cb0aa9))
* **update:** nudge shell-completion refresh after a real update ([#197](https://github.com/godaddy/cli/issues/197)) ([7b49a46](https://github.com/godaddy/cli/commit/7b49a4601e6d8385340db2c6678480b0c88c2675))


### Bug Fixes

* **cli:** render platform actions list as a table with pagination ([#198](https://github.com/godaddy/cli/issues/198)) ([3c7deca](https://github.com/godaddy/cli/commit/3c7deca563946ffa3563e56fbd93895ca079574f))
* **release:** update version argument support ([#195](https://github.com/godaddy/cli/issues/195)) ([2964db2](https://github.com/godaddy/cli/commit/2964db2fd567e5d389270f323b62c5afb810d0fe))


### Miscellaneous

* **rust:** enforce command-tree file structure; split oversized modules ([#192](https://github.com/godaddy/cli/issues/192)) ([0b5b131](https://github.com/godaddy/cli/commit/0b5b1312c65c8e9607754b8672ee1b3f6d60c594))

## [0.2.3](https://github.com/godaddy/cli/compare/v0.2.2...v0.2.3) (2026-08-10)


### Features

* **cli:** opt into cli-engine's pagination for domain/dns list and api search [DEVEX-972] ([#189](https://github.com/godaddy/cli/issues/189)) ([8482450](https://github.com/godaddy/cli/commit/8482450805073acc1050ca800ca7f6ba0f2ae3a4))

## [0.2.2](https://github.com/godaddy/cli/compare/v0.2.1...v0.2.2) (2026-08-06)


### Features

* **api-explorer:** add command hyperlinks for large output [DEVEX-967] ([#187](https://github.com/godaddy/cli/issues/187)) ([bfcc4be](https://github.com/godaddy/cli/commit/bfcc4be60083c11b0c797ff95024bf489170766c))
* **cli:** convert command args to cli-engine's typed-args API ([#181](https://github.com/godaddy/cli/issues/181)) ([b6bf793](https://github.com/godaddy/cli/commit/b6bf79300d04df023503628e1dd7c0785cd515c9))
* **output:** render nested arrays/objects as tables instead of raw JSON [DEVEX-968] ([#180](https://github.com/godaddy/cli/issues/180)) ([9c15fd2](https://github.com/godaddy/cli/commit/9c15fd295a51cbff52d6838dff624147a6f1f172))


### Bug Fixes

* **auth:** report per-environment status and refresh-token availability ([#184](https://github.com/godaddy/cli/issues/184)) ([cf7d9f5](https://github.com/godaddy/cli/commit/cf7d9f554c9812f19cab14576d46f081cbfbba50))
* **env:** add app config summary to env info [DEVEX-715] ([#185](https://github.com/godaddy/cli/issues/185)) ([9f85943](https://github.com/godaddy/cli/commit/9f85943e3f97165bc62d378b9e3ff720de390002))
* **extensions:** Rust bundler/deploy path parity with TS [DEVEX-712] ([#177](https://github.com/godaddy/cli/issues/177)) ([e995f99](https://github.com/godaddy/cli/commit/e995f990f00d5d1e4ec9afc44ac1631691d0a977))
* Node.js Hosting CLI deployments --limit flag typing issue ([#183](https://github.com/godaddy/cli/issues/183)) ([4028ff1](https://github.com/godaddy/cli/commit/4028ff1f612ddbf52d894e9c0ee0096af429ab7f))


### Miscellaneous

* **nodejs:** update --help descriptions for Node.js Hosting CLI com… ([#186](https://github.com/godaddy/cli/issues/186)) ([71ae7a5](https://github.com/godaddy/cli/commit/71ae7a5a55a863544a24725a927d45ac01ed8893))

## [0.2.1](https://github.com/godaddy/cli/compare/v0.2.0...v0.2.1) (2026-08-01)


### Bug Fixes

* **cli:** drop experimental platform suggestion, fix stale --search doc ([#174](https://github.com/godaddy/cli/issues/174)) ([48047d1](https://github.com/godaddy/cli/commit/48047d11c398833da755d35e5d6e7fcb799fa908))

## [0.2.0](https://github.com/godaddy/cli/compare/v0.1.16...v0.2.0) (2026-08-01)


### ⚠ BREAKING CHANGES

* **env:** `--search` is now a real `search <query...> [--scope <path>]` command (from cli-engine 0.5.0), not a global flag that could attach to (and silently swallow the output of) any other command. Per-environment override env vars are now app-scoped (`GDDY_AUTH_URL`, `GDDY_TOKEN_URL`, `GDDY_DOMAINS_API_URL`, `GDDY_ACCOUNT_URL`) instead of environment-scoped (`<ENV>_OAUTH_AUTH_URL`, `<ENV>_OAUTH_TOKEN_URL`, `<ENV>_DOMAINS_API_URL`, `<ENV>_ACCOUNT_URL`); `api_url` is no longer env-var overridable at all. An environment defined only via `<PREFIX>_API_URL`, with no compiled-in or `environments.toml` entry, no longer resolves — every environment must now be a builtin (`ote`/`prod`) or a real `environments.toml` entry. A malformed (non-blank) override, from an env var or `environments.toml`, is now a hard error instead of silently falling back to the derived default (blank/ whitespace-only overrides still fall back, unchanged). An unknown or misconfigured `--env` is now a hard error everywhere, instead of silently falling back to prod's API URL.

### Features

* **api:** restore agent-safe describe/list/search output [DEVEX-702] ([#169](https://github.com/godaddy/cli/issues/169)) ([1f33ec6](https://github.com/godaddy/cli/commit/1f33ec604cd9837fcd1d8ab4f16f8d3d19204135))
* **env:** migrate gddy onto cli-engine's declarative EnvConfig (DEVEX-947) ([#171](https://github.com/godaddy/cli/issues/171)) ([44f7368](https://github.com/godaddy/cli/commit/44f73684d92548c054b44c45dc8028c79215ff25))
* **webhook:** Normalize webhook output and truncate larger results ([#165](https://github.com/godaddy/cli/issues/165)) ([af59d11](https://github.com/godaddy/cli/commit/af59d11afc04bed03c64ef2dbdf80678a115ca4e))


### Bug Fixes

* Mapped response to { eventType, description } per event, added 50-item truncation with temp file fallback, and returned { events, total, shown, truncated, full_output } matching the original TS behavior ([af59d11](https://github.com/godaddy/cli/commit/af59d11afc04bed03c64ef2dbdf80678a115ca4e))


### Miscellaneous

* **cicd:** Add audit step and regenerate lock file ([#158](https://github.com/godaddy/cli/issues/158)) ([4ed50e8](https://github.com/godaddy/cli/commit/4ed50e8ba0c747744bccbdb78ca0f903759345cf))

## [0.1.16](https://github.com/godaddy/cli/compare/v0.1.15...v0.1.16) (2026-07-30)


### Features

* **errors:** map failures to stable codes and top-level fix [DEVEX-945] ([#162](https://github.com/godaddy/cli/issues/162)) ([576641c](https://github.com/godaddy/cli/commit/576641c9416cd2ec2cf85cb1d39d1bfd32ffb93f))


### Bug Fixes

* **api-catalog:** distinguish $defs keys for same-file property refs [DEVEX-965] ([#168](https://github.com/godaddy/cli/issues/168)) ([80d3abe](https://github.com/godaddy/cli/commit/80d3abebd25192df226f068f2e0fa6854df6ebd4))
* **api-catalog:** resync drift, normalize domains base URL, sort + env-aware routing ([#167](https://github.com/godaddy/cli/issues/167)) ([339db2d](https://github.com/godaddy/cli/commit/339db2dab8bea36d8e411dcc5699f184295e857a))
* **config:** restore godaddy.toml schema validation [DEVEX-714] ([#166](https://github.com/godaddy/cli/issues/166)) ([4cfa3e4](https://github.com/godaddy/cli/commit/4cfa3e4afd22684116e117f4313f58970514115c))

## [0.1.15](https://github.com/godaddy/cli/compare/v0.1.14...v0.1.15) (2026-07-28)


### Features

* **application init:** gate creation on onboarding (DEVX-545) ([#139](https://github.com/godaddy/cli/issues/139)) ([afa87e2](https://github.com/godaddy/cli/commit/afa87e235ad995d65907dbc8ac9673c708e621fd))
* **paas:** add GitHub operations for Node.js Hosting CLI command group ([#135](https://github.com/godaddy/cli/issues/135)) ([1de4b88](https://github.com/godaddy/cli/commit/1de4b881e281626a3b3dd764d1452efbf7f5e9d7))
* **platform:** namespace Developer Platform commands (DEVX-621) ([#149](https://github.com/godaddy/cli/issues/149)) ([faf89cc](https://github.com/godaddy/cli/commit/faf89cc8ee0ccbd82bfefda9535a569ee629ace0))


### Bug Fixes

* **onboarding:** send CLI User-Agent header ([#157](https://github.com/godaddy/cli/issues/157)) ([467c7f7](https://github.com/godaddy/cli/commit/467c7f74bd35948fddbfa6c02529ba6f85d4cbed))


### Code Refactoring

* improve agent UX for Node.js Hosting CLI commands ([#153](https://github.com/godaddy/cli/issues/153)) ([ff56ec6](https://github.com/godaddy/cli/commit/ff56ec6c90dbd9242d366202eafaa887794faa77))


### Tests

* **api:** cover commerce scope step-up inputs ([#150](https://github.com/godaddy/cli/issues/150)) ([403dc71](https://github.com/godaddy/cli/commit/403dc71674d1edb939c8dcf624ebbe352dc38ed8))

## [0.1.14](https://github.com/godaddy/cli/compare/v0.1.13...v0.1.14) (2026-07-23)


### Features

* **env:** consolidate onto cli-engine's shared Environments, wire feature-flag stages (DEVEX-929) ([#145](https://github.com/godaddy/cli/issues/145)) ([a6f48fe](https://github.com/godaddy/cli/commit/a6f48fe3dbbe4996e0ba6b540cc284637ffd698a))

## [0.1.13](https://github.com/godaddy/cli/compare/v0.1.12...v0.1.13) (2026-07-22)


### Features

* **dns:** add --replace-conflicting-types flag, fix CNAME conflict errors ([#102](https://github.com/godaddy/cli/issues/102)) ([2c0e576](https://github.com/godaddy/cli/commit/2c0e576048e393af8d35c78ed2bee6cbfa78af4f))
* **dry-run:** let dry-run validate input and preview real effects ([#141](https://github.com/godaddy/cli/issues/141)) ([6651c6b](https://github.com/godaddy/cli/commit/6651c6be293aa93fc48503a7486353d23d2aadc1))


### Bug Fixes

* **application validate:** restore remote application state checks ([#134](https://github.com/godaddy/cli/issues/134)) [DEVEX-708] ([b84ddb0](https://github.com/godaddy/cli/commit/b84ddb05c42c1b66e571818e3759f20e108347a0))
* **dns:** stop `dns set` from wiping zones via the v3 replace-record PUT ([#143](https://github.com/godaddy/cli/issues/143)) ([90c2408](https://github.com/godaddy/cli/commit/90c240831e20104a7dcc326fc3b7b0a57469af0e))

## [0.1.12](https://github.com/godaddy/cli/compare/v0.1.11...v0.1.12) (2026-07-20)


### Bug Fixes

* **pat:** stop over-specifying PAT shape in error and docs (DEVEX-889) ([#131](https://github.com/godaddy/cli/issues/131)) ([92144a3](https://github.com/godaddy/cli/commit/92144a3f3a1ac041b86ac26eb1a7eb06999d503d))

## [0.1.11](https://github.com/godaddy/cli/compare/v0.1.10...v0.1.11) (2026-07-20)


### Bug Fixes

* **api-explorer:** remove broken --query flag, add domains catalog source (DEVEX-898) ([#124](https://github.com/godaddy/cli/issues/124)) ([c07994a](https://github.com/godaddy/cli/commit/c07994a9fa7c141b10db610cca20eea383b7565e))
* **application update:** restore --status `ACTIVE | INACTIVE` ([#132](https://github.com/godaddy/cli/issues/132)) [DEVEX-709] ([565e0d7](https://github.com/godaddy/cli/commit/565e0d75d956d18823338605eac0198964167128))
* **extensions:** restore UI extension targets on add + deploy (DEVX-541) ([#107](https://github.com/godaddy/cli/issues/107)) ([247f712](https://github.com/godaddy/cli/commit/247f71254c1916b0126384e81af325863be84957))

## [0.1.10](https://github.com/godaddy/cli/compare/v0.1.9...v0.1.10) (2026-07-17)


### Bug Fixes

* **application init:** align with TS config seeding and validation ([#126](https://github.com/godaddy/cli/issues/126)) (DEVEX-707) ([1cfcda2](https://github.com/godaddy/cli/commit/1cfcda210edb305615a92ae433e11833392bcd85))
* **domain:** validate domain and nameserver hostname shape before the API call ([#127](https://github.com/godaddy/cli/issues/127)) ([9348495](https://github.com/godaddy/cli/commit/934849587684cc4f221b94bae1f162c95aa69a66))

## [0.1.9](https://github.com/godaddy/cli/compare/v0.1.8...v0.1.9) (2026-07-17)


### Bug Fixes

* **application deploy:** guarantee a terminal result/error NDJSON event (DEVX-544) ([#114](https://github.com/godaddy/cli/issues/114)) ([1d0fb2d](https://github.com/godaddy/cli/commit/1d0fb2d3d04b9ef5201d0322b2104e6e638f4573))
* **domain:** show renewal price consistently across available/quote/suggest (GDDEVPLAT-133) ([#125](https://github.com/godaddy/cli/issues/125)) ([71e4eea](https://github.com/godaddy/cli/commit/71e4eea5da9a6d8a547560e40b211229f9213166))

## [0.1.8](https://github.com/godaddy/cli/compare/v0.1.7...v0.1.8) (2026-07-17)


### Bug Fixes

* **domain suggest:** validate --limit against the v3 API's 50-suggestion cap (DEVEX-883) ([#117](https://github.com/godaddy/cli/issues/117)) ([c409719](https://github.com/godaddy/cli/commit/c409719be3344ede4eb3ffe1e487c13e4f023f1a))

## [0.1.7](https://github.com/godaddy/cli/compare/v0.1.6...v0.1.7) (2026-07-17)


### Features

* **auth:** add live OAuth scope registry via `gddy auth scopes` (DEVEX-886, DEVEX-891) ([#116](https://github.com/godaddy/cli/issues/116)) ([cb16cc9](https://github.com/godaddy/cli/commit/cb16cc9dfa1fcc717e71b70a5c94e41e0c787bcf))

## [0.1.6](https://github.com/godaddy/cli/compare/v0.1.5...v0.1.6) (2026-07-16)


### Bug Fixes

* **payment-methods:** rename gddy payments to gddy payment-methods (DEVEX-900) ([#119](https://github.com/godaddy/cli/issues/119)) ([67512bc](https://github.com/godaddy/cli/commit/67512bc39b91a4a45e360ac127b7165c82124439))

## [0.1.5](https://github.com/godaddy/cli/compare/v0.1.4...v0.1.5) (2026-07-16)


### Bug Fixes

* **auth:** validate requested OAuth scopes against the CLI's registry (DEVEX-894) ([#108](https://github.com/godaddy/cli/issues/108)) ([6abd2ee](https://github.com/godaddy/cli/commit/6abd2ee7b52be482f68a137a8527dc469da261a0))

## [0.1.4](https://github.com/godaddy/cli/compare/v0.1.3...v0.1.4) (2026-07-15)


### Features

* **api-catalog:** add drift guards for the embedded catalog (DEVX-549) ([#98](https://github.com/godaddy/cli/issues/98)) ([3605d8e](https://github.com/godaddy/cli/commit/3605d8e96a8c4c895da1821e1258c8f01bec8254))


### Bug Fixes

* **api-call:** apply headers, error on non-2xx, GraphQL errors, output shape (DEVX-546) ([#97](https://github.com/godaddy/cli/issues/97)) ([29529c6](https://github.com/godaddy/cli/commit/29529c61dcdcbced7a6214616f19b31153acc2c5))
* **api-catalog:** resolve discriminator.mapping refs to #/$defs pointers (DEVX-548) ([#96](https://github.com/godaddy/cli/issues/96)) ([0b9d565](https://github.com/godaddy/cli/commit/0b9d56542b84e3e93bb0497f5c99291e6237f0eb))
* **application deploy:** activate release and promote app to ACTIVE (DEVEX-704) ([#103](https://github.com/godaddy/cli/issues/103)) ([87457ef](https://github.com/godaddy/cli/commit/87457ef6545a51db005205972f2436068604284d))
* **domain:** comma-join repeatable TLD flags before sending as query params (DEVEX-882) ([#106](https://github.com/godaddy/cli/issues/106)) ([ada4a93](https://github.com/godaddy/cli/commit/ada4a93700f9a9d80e5bf8b8b1adb3dcc8aa6ac3))
* **release:** include actions, subscriptions, UI extensions in release (DEVX-540) ([#100](https://github.com/godaddy/cli/issues/100)) ([afd8d9b](https://github.com/godaddy/cli/commit/afd8d9beda13a130bc959cdbfc1925b251ec9e1b))

## [0.1.3](https://github.com/godaddy/cli/compare/v0.1.2...v0.1.3) (2026-07-11)


### Features

* **domain list:** hide cancelled/non-visible domains by default ([#94](https://github.com/godaddy/cli/issues/94)) ([ad17cd3](https://github.com/godaddy/cli/commit/ad17cd33f1f0c54723b97ee718a5fc48b856211a))

## [0.1.2](https://github.com/godaddy/cli/compare/v0.1.1...v0.1.2) (2026-07-10)


### Features

* flatten domain suggest term pricing and prefix next-actions with gddy ([#90](https://github.com/godaddy/cli/issues/90)) ([6f7a7a9](https://github.com/godaddy/cli/commit/6f7a7a9f2a13536a901f827c21e9804386f18812))


### Bug Fixes

* use non-API GitHub redirect for update checks, add --force to update apply ([#91](https://github.com/godaddy/cli/issues/91)) ([60cc92d](https://github.com/godaddy/cli/commit/60cc92d7a325b7d65350b2599e494b716bc1d222))

## [0.1.1](https://github.com/godaddy/cli/compare/v0.1.0...v0.1.1) (2026-07-09)


### Features

* add `domain operation status` to poll async domain operations ([#86](https://github.com/godaddy/cli/issues/86)) ([4ad277c](https://github.com/godaddy/cli/commit/4ad277c0b64cc2bae4707943c03b23adae195fe2))
* add gddy payments add command ([f28deb2](https://github.com/godaddy/cli/commit/f28deb21ec089a7ef19e6b0a0c2d33ebb5ea866a))
* add gddy payments add command ([42e3be6](https://github.com/godaddy/cli/commit/42e3be6e86465e8c1d0948c9f5808ed799bc099c))
* add more next-actions across commands ([#83](https://github.com/godaddy/cli/issues/83)) ([a62460b](https://github.com/godaddy/cli/commit/a62460bf3091a7db695da63848295eb9b8b1ad1c))
* add Personal Access Token (PAT) support ([#80](https://github.com/godaddy/cli/issues/80)) ([827d448](https://github.com/godaddy/cli/commit/827d4485b2e7c1f90d2b546962c0d32d21d0bfe3))
* default to prod, fix cross-platform config writes, support Windows installs ([#54](https://github.com/godaddy/cli/issues/54)) ([43ad0ad](https://github.com/godaddy/cli/commit/43ad0ade523d26ff61851047ff5fc9128c441bf0))
* DEV/TEST environments + OAuth scope step-up (DEVEX-719) ([#57](https://github.com/godaddy/cli/issues/57)) ([32cd77e](https://github.com/godaddy/cli/commit/32cd77e3ed66e3e0f183f9b8e627c2e18fb090be))
* DNS record management + domain list, with output-field discovery ([#65](https://github.com/godaddy/cli/issues/65)) ([fa66cfa](https://github.com/godaddy/cli/commit/fa66cfa34b20eff2687f1f6e84510bcc6c1dfa51))
* domain availability + suggest via spec-generated client ([#59](https://github.com/godaddy/cli/issues/59)) ([f56cd21](https://github.com/godaddy/cli/commit/f56cd21b47856ce2792a2000a6209cfc6ed7e549))
* domain purchase (v2 register) + domain get ([#70](https://github.com/godaddy/cli/issues/70)) ([96696ca](https://github.com/godaddy/cli/commit/96696ca8c99d506261f727954540dd28c414649f))
* integrate Node.js Hosting public API with new CLI ([#78](https://github.com/godaddy/cli/issues/78)) ([8877abe](https://github.com/godaddy/cli/commit/8877abe53be39da35e3e4c8a91390097530eb5a0))
* migrate domain & dns commands to the v3 Domains API ([#76](https://github.com/godaddy/cli/issues/76)) ([c39fbeb](https://github.com/godaddy/cli/commit/c39fbebdacd51033e9cf6e0b94c75c9b95be74c3))
* publish gddy alpha binary on rust-port pushes; rename binary to gddy ([#53](https://github.com/godaddy/cli/issues/53)) ([9a37bde](https://github.com/godaddy/cli/commit/9a37bdeaebea61f87a6c08c4909fefbabcd9e180))
* **release:** release-please, self-update, no-sudo installs ([#88](https://github.com/godaddy/cli/issues/88)) ([e66f990](https://github.com/godaddy/cli/commit/e66f990f4fb504df743559058ade23d4d210e32f))
* reorganize module categories and set feature-flag stages ([#87](https://github.com/godaddy/cli/issues/87)) ([7ddf2c4](https://github.com/godaddy/cli/commit/7ddf2c41f196be6a901728ca61394e61593d44ea))


### Bug Fixes

* address PR review — account_url in environments module, URL on browser failure ([e5966ed](https://github.com/godaddy/cli/commit/e5966ed70dfe4c04902be83647f4a464ef60d305))
* adopt cli-engine 0.2.0 fail-closed auth; mark local commands no_auth ([#56](https://github.com/godaddy/cli/issues/56)) ([1cc8d98](https://github.com/godaddy/cli/commit/1cc8d9878e821455c900f33d50b174953621c5b7))
* domain-purchase data formatting + comprehensive CLI help ([#75](https://github.com/godaddy/cli/issues/75)) ([5d15d6d](https://github.com/godaddy/cli/commit/5d15d6d4b499c34c68ba21854ba719af293355ee))
* payments add always returns URL, treats browser failure as non-fatal ([#72](https://github.com/godaddy/cli/issues/72)) ([07cba90](https://github.com/godaddy/cli/commit/07cba903693f322e22b9f1fc228ebc5f0ae5a404))
* request offline_access scope for refresh tokens, centralize hosting scopes ([#85](https://github.com/godaddy/cli/issues/85)) ([b6d49c7](https://github.com/godaddy/cli/commit/b6d49c7c7e0a29e8c3b9808332efe597008367ca))
* route all HTTP clients through --debug transport logger ([#81](https://github.com/godaddy/cli/issues/81)) ([d0c94a3](https://github.com/godaddy/cli/commit/d0c94a305b1792073d3a7760ce4a6ac3616ddc11))
* stop truncating agreement URLs in `domain agreements` human output ([#82](https://github.com/godaddy/cli/issues/82)) ([6cf9112](https://github.com/godaddy/cli/commit/6cf911265f063ce164aab92e16e619fb9f379ca5))
* surface failure detail on domain purchase, unify cli-engine version ([#84](https://github.com/godaddy/cli/issues/84)) ([d3a9cd1](https://github.com/godaddy/cli/commit/d3a9cd1930167f2f947987c292ed280205fae82d))


### Miscellaneous

* bump cli-engine to 0.3.4 for non-interactive scope step-up ([#73](https://github.com/godaddy/cli/issues/73)) ([206c2b3](https://github.com/godaddy/cli/commit/206c2b3277478e69d46019681e0bb53e38578943))
