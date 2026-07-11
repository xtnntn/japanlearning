# 日语阅读日报

面向中文母语者的个人 AI 日语阅读训练器。它每天只提供一篇与你兴趣和能力相匹配的真实日文文章，帮助你逐步摆脱中文翻译，独立读懂 ACGN、VTuber 等题材的内容。

当前第一服务对象是开发者本人，第一终端是 macOS 桌面应用。

## MVP 已实现

- Tauri 2 + React + Rust 的 macOS 桌面应用；
- 从 [KAI-YOU](https://kai-you.net/) 拉取公开文章，并在干净阅读器中展示；
- 过滤会员提示、用户投稿和过短正文，保留原图及第三方视频/社媒来源；
- 文章内任意划词：先显示简短日语提示，按需展开中文；
- 需要时可主动请求深入解释，默认划词请求不会自动变长；
- OpenAI API Key 未配置时提供可用的本地降级解释；配置后可选择 Responses 或 Chat Completions 生成结构化解释；
- API Key 保存到 macOS Keychain，不写入前端、Git 或 SQLite；
- 到达文末后自动出现带原文证据的理解题；
- 首次标题兴趣校准（想看 / 无感 / 不想看）；
- 每日候选会结合标题投票、题材反馈与近期阅读难度排序，每周二、周五保留探索题材；
- 首次阅读与词汇定位测试，建立起始能力画像；
- 每周独立阅读评估，评估时禁用划词和中文翻译；
- 可由用户主动启用的 macOS 每日标题提醒，支持修改时间和关闭；
- 最近 14 天按文章长度、难度校正的划词频率趋势，并单列独立评估成绩；
- 两周实验状态在样本不足时拒绝下结论，并分别显示三项验收证据；
- 新语境旧表达证据链：历史划词在新文章重现时，可由理解题绑定验证独立理解率；
- 可查看的基础能力画像，以及只影响未来选文的目标难度人工校正；
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
当前实现证据与剩余验证见 [docs/MVP实现审计.md](docs/MVP实现审计.md)。

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

应用右上角点击“AI 设置”，输入兼容服务的 Base URL 和 API Key，检测服务实际提供的模型，选择后保存。

Base URL、调用协议、模型与 Key 会进入 macOS Keychain。选择 `Responses` 时调用 `/responses`；选择 `Chat Completions` 时调用 `/chat/completions`。前端不会回显已保存的 Key；修改 Base URL、协议或模型时可以留空复用现有 Key。不要将 Key 写入 `.env`、源码、SQLite 或 Git 提交。

每日提醒也在“AI 设置”中配置。只有点击“启用提醒”后，应用才会写入用户级 `launchd` 配置；关闭提醒会移除该配置。

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

当前两周试验包 SHA-256：

```text
9994ac5f48686e21fe2acdf32e7b49df2c59e912762ff7e2e37b2662c157c87fbf
```

当前配置使用完整的本地 ad-hoc 签名，适合开发者本人安装和更新。它没有 Apple Developer ID 公证；若未来对外分发，需要配置正式证书并完成 notarization。

## 验证命令

```bash
npm run build
(cd src-tauri && cargo check)
(cd src-tauri && cargo test)
```

KAI-YOU 当前网站结构的实时冒烟测试默认忽略，按需运行：

```bash
(cd src-tauri && cargo test live_kaiyou_pages_have_at_least_one_compatible_article -- --ignored)
```

## 当前限制与下一步

- 当前个性化选文使用本地可解释特征排序，尚未加入 LLM 语义主题分类；
- 旧表达验证依赖 AI 生成的证据题，尚需真实两周数据验证题目覆盖率；
- 当前只支持 KAI-YOU 单一来源；
- Anki 式间隔复习和 AI 复现句按范围取舍暂缓；
- 未来若面向其他用户，必须切换到授权、开放许可或合作内容，不能复用个人模式的临时全文阅读策略。
