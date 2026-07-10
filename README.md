# 日语阅读日报

面向中文母语者的个人 AI 日语阅读训练器。它每天只提供一篇与你兴趣和能力相匹配的真实日文文章，帮助你逐步摆脱中文翻译，独立读懂 ACGN、VTuber 等题材的内容。

当前第一服务对象是开发者本人，第一终端是 macOS 桌面应用。

## MVP 已实现

- Tauri 2 + React + Rust 的 macOS 桌面应用；
- 从 [KAI-YOU](https://kai-you.net/) 拉取公开文章，并在干净阅读器中展示；
- 文章内任意划词：先显示简短日语提示，按需展开中文；
- OpenAI API Key 未配置时提供可用的本地降级解释；配置后使用 Responses API 生成结构化解释；
- API Key 保存到 macOS Keychain，不写入前端、Git 或 SQLite；
- 到达文末后自动出现带原文证据的理解题；
- 首次标题兴趣校准（想看 / 无感 / 不想看）；
- 首次阅读与词汇定位测试，建立起始能力画像；
- SQLite 本地保存阅读、划词、题目、兴趣与画像数据；
- 旧文章正文与图片缓存会在次日清除，只保留必要的学习记录；
- 已生成 macOS `.app` 与 `.dmg` 安装包。

## 设计原则

- 每天只有一篇主阅读文章；未完成任务次日过期，不形成补读债务。
- 原文不被改写，题目答案必须能定位回原文证据。
- 翻译是安全网，而不是第一反应：日语释义优先，中文由用户主动展开。
- 兴趣探索与已验证兴趣并存：计划每周两篇探索题材。
- 衡量进步不能只看划词减少；需要同时观察无翻译理解和新语境中旧表达的独立理解率。

完整决策见 [docs/产品决策记录.md](docs/产品决策记录.md)。

## 开发环境

- macOS
- Node.js 20+
- Rust stable
- Xcode Command Line Tools

## 本地运行

```bash
npm install
npm run tauri dev
```

开发态首次启动会：

1. 展示若干 KAI-YOU 标题，快速校准你的兴趣；
2. 进行一轮初始阅读/词汇定位；
3. 进入当天的唯一阅读文章。

## 配置 OpenAI

应用右上角点击“AI 设置”，粘贴 OpenAI API Key 并保存。

Key 会进入 macOS Keychain，之后由 Rust 后端读取并调用 OpenAI Responses API。不要将 Key 写入 `.env`、源码、SQLite 或 Git 提交。

未配置 Key 时，应用仍可读取文章、记录划词和进行本地保守题目；但针对划词的个性化解释与高质量证据题需要配置 Key。

## 构建 macOS 安装包

```bash
npm run tauri build
```

构建结果位于：

```text
src-tauri/target/release/bundle/macos/日语阅读日报.app
src-tauri/target/release/bundle/dmg/日语阅读日报_0.1.0_aarch64.dmg
```

## 验证命令

```bash
npm run build
(cd src-tauri && cargo check)
```

## 当前限制与下一步

- 每日定时通知的 `launchd` 安装流程尚未完成；
- 每周无翻译独立阅读评估尚待接入；
- 当前只支持 KAI-YOU 单一来源；
- Anki 式间隔复习和 AI 复现句按范围取舍暂缓；
- 未来若面向其他用户，必须切换到授权、开放许可或合作内容，不能复用个人模式的临时全文阅读策略。
