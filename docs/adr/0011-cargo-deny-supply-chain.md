# ADR 0011: cargo-deny 供应链安全

- **状态:** Accepted
- **日期:** 2026-08-06
- **决策者:** VoicePilot Team
- **标签:** supply-chain, security

## 背景

VoicePilot 需要依赖供应链安全检查(许可证 / 漏洞 / 多版本冲突 / 来源)。选择何种工具?

## 决策

使用 cargo-deny(`deny.toml`)做依赖供应链安全。

- `[graph]`:x86_64-pc-windows-msvc only + all-features
- `[advisories]`:unmaintained=workspace / unsound=all
- `[bans]`:multiple-versions=warn + wildcards=deny;deny git2/openssl/libssh2-sys
- `[sources]`:unknown-registry=deny / unknown-git=deny
- `[licenses]`:allow MIT/Apache-2.0/ISC/Zlib/BSD/MPL-2.0 等
- `.github/workflows/ci.yml` lint job 集成

## 替代方案

- **cargo-audit 单独:** 拒绝。仅覆盖漏洞,不覆盖许可证 / 多版本 / 来源。
- **无工具:** 拒绝。无法保证供应链安全。

## 后果

- <正向:许可证 + 漏洞 + 多版本 + 来源一站式检查,CI 门禁>
- <负向:需维护 deny.toml 的 skip / exceptions>
- <中性:与 cargo-audit(Plan 6 周审计)互补>

## 参考

- W12 设计文档 `docs/superpowers/specs/2026-08-06-w12-engineering-standards-design.md`
- `deny.toml` + `.github/workflows/ci.yml`