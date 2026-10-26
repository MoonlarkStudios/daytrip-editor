# Public history references

Before the first public push, commit author and committer identities were updated
to John Slaughter <john@moonlarkstudios.com> on 2026-10-25. Every rewritten commit
retains its original file tree, timestamp, message and parent ordering. No source
or benchmark output was changed by that metadata operation.

Narrative documentation now uses the public commit IDs. Validation logs retain
the IDs recorded at measurement time; use this table to resolve those references.
File checksums and executable hashes are independent of this mapping.

| Recorded commit | Public commit | Change |
| --- | --- | --- |
| `284087cc205eb07b7a0564bee462d8280664f9cf` | `e4ea4aa8867d751e5a7a9a69bcb8537d415c9430` | feat(editor): establish the live markdown editor baseline |
| `37318234736c1f42c6ddbc11b8340c8b8c89ca20` | `9c59410bb6b92b2dbb70bcf4cbc763ee7df7f938` | refactor: organize source modules by responsibility |
| `63e4ae76be51159ae6e5faca396c93df200115de` | `de2e07588fe3956411196da695a1c2b097cf33e3` | docs: define Rust and contribution conventions |
| `ebdcdedf70c8f695d48d55c0d91e464191df862d` | `a0b3714cce5bcf5a93f0b0109e11f245c15e08e9` | perf(editor): virtualize rich layout around the viewport |
| `bbbeb0fe4ac097eca18a97750130364510643e40` | `b490dd25f80b25a27d323637c837639c0315c88d` | perf(editor): defer native buffer shaping during document load |
| `5144f5098c8703d2d4c4b45b7659c3a7b860bea9` | `10507ff6792acfa012b56e31274b475770a311da` | perf(document): parse isolated Markdown blocks incrementally |
| `8e861708608ea369b4bb7588fd16f72ffe8985c2` | `740fc006804cab04788cdcea0a4a38c1ba3446fd` | perf(editor): reuse native buffer snapshots across edits |
| `26dcedac353205b5968baf9cb3c486ea5650ad50` | `fecbc482279bb5e4c6b9fd0516f44270bcb97fc9` | perf(document): update source and projection buffers in place |
| `8622a46e1c816cc107a3457df1a8f582abf9e184` | `81d1f1ca39571588d780e186690e48fca9ad07bc` | feat(document): integrate CRDT editing and offline collaboration |
| `21df3848f4bef96018e80333c81bf081431c911c` | `229d89d753b5e86834a838e7e741359a9453e39c` | perf(document): reuse cached Yrs UTF-16 item lengths |
| `6a2d74335021c3a8dcfc72a821f30fafe50e305e` | `34d6cc30114ab748958ca116de0134d226047c9b` | perf(document): incrementally apply offline imports |
| `20828c32ecbf4e4caf151c720360142be8d6f723` | `e73e4c084cb6e15e3789cd5b53204e1b0dac7633` | feat(writing): polish media, links, credits, and document care |
| `97cd2fe53eee60ab9ec88ce66b46b539f0050018` | `901cc60572b13aaa15de5629866e21893e726421` | fix(editor): refresh canceled composition targets |
| `3f67fb16b6921c73c4e45e9532d9c53c4d1a9868` | `39fada45605ebc1cccd3a3769c4fded9617365f7` | docs: record native PNG drop validation |
| `6487f7786aed948dde47650b1f6831a8f6f7ad6c` | `f0205b55341c3f360fa7a88facb321c05be9b0f3` | feat(branding): adopt Daytrip and the open book logo |
| `46f3bd2318d2ea37088c7e18314ac64d49f75826` | `bfefa96a97739219ee84a975dde12c4737f817a7` | feat(theme): integrate Daytrip branding across the application |
| `a1e148149ec985342662a0e3844f7463e72c1794` | `c490f46d7b9432b72eecbc28098c4c77b18f6072` | feat(editor): improve code block rendering and editing |
| `1c219ceb34436a9de592fbe46dd51fdad8872023` | `4c791c7d7e55e6c92179481b98c9b1f43df8b1e2` | feat(outline): add heading navigation and atomic section moves |
| `9ed6f5647f45e5618438ad241be12ebe2ff14cb5` | `b63cc86ac896d3a9dd10fbd8e7b2340d40c6c3dd` | build(vendor): scope upstream diagnostics and record test limits |
| `882d340195c595692b203dcee85f9e49e9932d2b` | `31ea05beac359f119d06382d7fd36d2abbc6272b` | fix(deps): update TLS trust and Windows clipboard dependencies |
| `e852803074030bb3cd8dc361123abf6e2b5522f6` | `73dbdd55291f130c9d83c01d106411bef5d08e6c` | build(repo): enforce publication checks and refresh documentation |

## Application identity in retained evidence

During publication preparation, application-name labels in historical text logs
were normalized to Daytrip at the repository owner's request. Each edited log
has a notice at the top. Measurements, test results, commit IDs and recorded hashes
were preserved. Package names, executable paths, environment variables and trace
module labels now use the public identity, so these files are edited evidence,
not byte-for-byte terminal captures. A recorded executable hash describes the
original binary, not a newly built binary with the normalized filename.

The unedited evidence is recoverable from commit `29ebbe2`, immediately before
the identity cleanup. Reproduction commands in narrative documentation use the
current package/environment names; historical revisions used their own build
configuration. The data-path transition is described in
[manual migration](identity-migration.md).
