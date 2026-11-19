# Keeping up with upstream ProseCraft

ProseCraft is active. prosecraft-rs ports from a fixed upstream commit and catches up deliberately.

- **Baseline:** `7dae544bfaa1e077b03d1b8a5dfe38a190ff8fb5` (ProseCraft v0.6.0, 2026-11-15).
- **Each ported module** names its upstream file(s) and the commit it was ported from in its module docs.
- **Catching up:** list the upstream commits since the last recorded one (`git log <last>..upstream/main`) and add the ones that change behaviour or file formats to the table below. Port them, or mark them as not applicable.

| Upstream commit | What changed | prosecraft-rs |
|---|---|---|
| `8cdc694` | The local MCP server (`mcp/`) as of this commit | Ported as `apps/prosecraft-mcp` |
