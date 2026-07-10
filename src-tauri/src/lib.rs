use chrono::Local;
use reqwest::Client;
use rusqlite::{params, Connection};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::PathBuf, sync::Mutex};
use tauri::{Manager, State};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
enum AppError {
  #[error("{0}")]
  Message(String),
  #[error(transparent)]
  Database(#[from] rusqlite::Error),
  #[error(transparent)]
  Network(#[from] reqwest::Error),
}

impl serde::Serialize for AppError {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    serializer.serialize_str(&self.to_string())
  }
}

struct AppState {
  db_path: Mutex<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Article {
  id: String,
  title: String,
  source: String,
  url: String,
  published_at: String,
  paragraphs: Vec<String>,
  images: Vec<String>,
  reading_minutes: u32,
  difficulty: String,
  is_exploration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TitleCandidate {
  id: String,
  title: String,
  url: String,
  source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssessmentQuestion {
  id: String,
  prompt: String,
  choices: Vec<String>,
  answer_index: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssessmentResult {
  score: usize,
  total: usize,
  level_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Explanation {
  japanese_hint: String,
  chinese_translation: String,
  furigana: String,
  note: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiStatus {
  configured: bool,
  model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Question {
  id: String,
  prompt: String,
  choices: Vec<String>,
  answer_index: usize,
  evidence: String,
  explanation: String,
}

#[derive(Debug, Deserialize)]
struct QuestionSet {
  questions: Vec<Question>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
  selected_count: i64,
  chinese_reveals: i64,
  completed_articles: i64,
  title_votes: i64,
  baseline_completed: bool,
  topic_feedback: Vec<TopicFeedback>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TopicFeedback {
  label: String,
  count: i64,
}

fn open_db(state: &AppState) -> Result<Connection, AppError> {
  let path = state
    .db_path
    .lock()
    .map_err(|_| AppError::Message("无法访问本地学习数据库".into()))?
    .clone();
  let conn = Connection::open(path)?;
  conn.execute_batch(
    "
      PRAGMA busy_timeout=5000;
      PRAGMA journal_mode=WAL;
      CREATE TABLE IF NOT EXISTS articles (
        id TEXT PRIMARY KEY,
        day TEXT NOT NULL,
        title TEXT NOT NULL,
        source TEXT NOT NULL,
        url TEXT NOT NULL,
        published_at TEXT NOT NULL,
        paragraphs_json TEXT NOT NULL,
        images_json TEXT NOT NULL DEFAULT '[]',
        reading_minutes INTEGER NOT NULL,
        difficulty TEXT NOT NULL,
        is_exploration INTEGER NOT NULL DEFAULT 0,
        completed_at TEXT
      );
      CREATE TABLE IF NOT EXISTS selections (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        selection TEXT NOT NULL,
        context TEXT NOT NULL,
        chinese_revealed INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS topic_feedback (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        label TEXT NOT NULL,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS answers (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        question_id TEXT NOT NULL,
        chosen_index INTEGER NOT NULL,
        correct INTEGER NOT NULL,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS title_votes (
        id TEXT PRIMARY KEY,
        candidate_id TEXT NOT NULL,
        title TEXT NOT NULL,
        url TEXT NOT NULL,
        vote TEXT NOT NULL,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS assessments (
        id TEXT PRIMARY KEY,
        mode TEXT NOT NULL,
        score INTEGER NOT NULL,
        total INTEGER NOT NULL,
        completed_at TEXT NOT NULL
      );
    ",
  )?;
  let image_column_exists: i64 = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = 'images_json'",
    [],
    |row| row.get(0),
  )?;
  if image_column_exists == 0 {
    conn.execute("ALTER TABLE articles ADD COLUMN images_json TEXT NOT NULL DEFAULT '[]'", [])?;
  }
  Ok(conn)
}

fn api_key_entry() -> Result<keyring::Entry, AppError> {
  keyring::Entry::new("com.xtnntn.nihongo-daily-reader", "openai-api-key")
    .map_err(|error| AppError::Message(format!("无法访问 macOS Keychain：{error}")))
}

fn get_api_key() -> Option<String> {
  api_key_entry().ok()?.get_password().ok().filter(|key| !key.trim().is_empty())
}

async fn generate_structured<T: for<'de> Deserialize<'de>>(
  api_key: &str,
  system: &str,
  user: &str,
  schema_name: &str,
  schema: serde_json::Value,
) -> Result<T, AppError> {
  let payload = serde_json::json!({
    "model": "gpt-5.6-luna",
    "reasoning": { "effort": "low" },
    "input": [
      { "role": "system", "content": [{ "type": "input_text", "text": system }] },
      { "role": "user", "content": [{ "type": "input_text", "text": user }] }
    ],
    "text": {
      "format": {
        "type": "json_schema",
        "name": schema_name,
        "strict": true,
        "schema": schema
      }
    }
  });
  let response = Client::new()
    .post("https://api.openai.com/v1/responses")
    .bearer_auth(api_key)
    .json(&payload)
    .send()
    .await?;
  if !response.status().is_success() {
    return Err(AppError::Message(format!("OpenAI 请求失败：{}", response.status())));
  }
  let body: serde_json::Value = response.json().await?;
  let output = body
    .get("output_text")
    .and_then(|value| value.as_str())
    .ok_or_else(|| AppError::Message("OpenAI 未返回结构化文本".into()))?;
  serde_json::from_str(output).map_err(|error| AppError::Message(format!("AI 返回格式无效：{error}")))
}

fn explanation_schema() -> serde_json::Value {
  serde_json::json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "japaneseHint": { "type": "string" },
      "chineseTranslation": { "type": "string" },
      "furigana": { "type": "string" },
      "note": { "type": "string" }
    },
    "required": ["japaneseHint", "chineseTranslation", "furigana", "note"]
  })
}

fn question_schema() -> serde_json::Value {
  serde_json::json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "questions": {
        "type": "array",
        "minItems": 3,
        "maxItems": 3,
        "items": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "id": { "type": "string" },
            "prompt": { "type": "string" },
            "choices": { "type": "array", "minItems": 4, "maxItems": 4, "items": { "type": "string" } },
            "answerIndex": { "type": "integer", "minimum": 0, "maximum": 3 },
            "evidence": { "type": "string" },
            "explanation": { "type": "string" }
          },
          "required": ["id", "prompt", "choices", "answerIndex", "evidence", "explanation"]
        }
      }
    },
    "required": ["questions"]
  })
}

fn initial_assessment() -> Vec<AssessmentQuestion> {
  vec![
    AssessmentQuestion { id: "v1".into(), prompt: "「発売」の読み方として正しいものはどれですか。".into(), choices: vec!["はつばい".into(), "はっばい".into(), "はつうり".into(), "はつまい".into()], answer_index: 0 },
    AssessmentQuestion { id: "v2".into(), prompt: "「配信を楽しみにしています」の意味に最も近いものはどれですか。".into(), choices: vec!["我正在期待直播。".into(), "我不想看直播。".into(), "我已经结束直播。".into(), "我在制作直播。".into()], answer_index: 0 },
    AssessmentQuestion { id: "g1".into(), prompt: "次の文の（　）に入る最も自然なものはどれですか。\n新しい情報は、あとで確認する（　）メモしておこう。".into(), choices: vec!["ために".into(), "ながら".into(), "ので".into(), "しか".into()], answer_index: 0 },
    AssessmentQuestion { id: "g2".into(), prompt: "「まだ見ていない」の意味として正しいものはどれですか。".into(), choices: vec!["还没有看。".into(), "已经看完。".into(), "绝对不看。".into(), "正在看。".into()], answer_index: 0 },
    AssessmentQuestion { id: "g3".into(), prompt: "次の文とほぼ同じ意味の文を選んでください。\nこの記事は、初心者でも読めるように書かれている。".into(), choices: vec!["初めて読む人にも分かるように書かれている。".into(), "専門家だけのために書かれている。".into(), "読むことが禁止されている。".into(), "内容が全くない。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r1".into(), prompt: "次の文章を読んでください。\n「イベントは予定より一時間遅れて始まった。しかし、出演者が登場すると、会場はすぐに盛り上がった。」\nイベントについて正しいものはどれですか。".into(), choices: vec!["開始は予定より遅かった。".into(), "出演者は来なかった。".into(), "会場は最初から静かだった。".into(), "イベントは中止になった。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r2".into(), prompt: "同じ文章について、「しかし」が表す関係として最も近いものはどれですか。".into(), choices: vec!["前の内容とは違う展開".into(), "同じ内容の繰り返し".into(), "理由の説明".into(), "時間の順番".into()], answer_index: 0 },
    AssessmentQuestion { id: "r3".into(), prompt: "次の文章を読んでください。\n「公式サイトで発表された内容によると、新作ゲームの発売日は来月に変更された。開発チームは品質を上げるために、もう少し時間が必要だと説明している。」\n発売日が変更された主な理由は何ですか。".into(), choices: vec!["品質を上げるため".into(), "公式サイトが閉じたため".into(), "ゲームが完成したため".into(), "来月が休みのため".into()], answer_index: 0 },
    AssessmentQuestion { id: "r4".into(), prompt: "同じ文章で「によると」はどのような働きですか。".into(), choices: vec!["情報の出所を示す".into(), "命令を表す".into(), "比較を表す".into(), "質問を表す".into()], answer_index: 0 },
    AssessmentQuestion { id: "r5".into(), prompt: "「〜わけではない」を使った文として正しい意味を選んでください。\n日本のアニメなら、何でも好きなわけではない。".into(), choices: vec!["所有日本动画我都喜欢，并不是这个意思。".into(), "我完全不喜欢日本动画。".into(), "我只看日本动画。".into(), "日本动画不存在。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r6".into(), prompt: "次の文で、筆者の意見を表している部分はどれですか。\n「この作品は話題になっているが、私は物語より音楽のほうが印象に残った。」".into(), choices: vec!["私は物語より音楽のほうが印象に残った。".into(), "この作品は話題になっている。".into(), "作品という言葉。".into(), "なっているという表現。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r7".into(), prompt: "次の文章の要点として最も近いものを選んでください。\n「ニュースを読むとき、知らない単語だけに注目すると、記事全体の意味を見失いやすい。まず見出しと段落の流れをつかみ、必要な表現だけを確認するほうが理解しやすい。」".into(), choices: vec!["先に全体の流れをつかむことが大切だ。".into(), "すべての単語を辞書で調べるべきだ。".into(), "見出しは読まなくてよい。".into(), "ニュースは読まないほうがよい。".into()], answer_index: 0 },
  ]
}

fn sample_article() -> Article {
  Article {
    id: "sample-kaiyou-reader".into(),
    title: "ポップカルチャーを読むための小さな入口".into(),
    source: "KAI-YOU（接続確認用サンプル）".into(),
    url: "https://kai-you.net/".into(),
    published_at: Local::now().format("%Y-%m-%d").to_string(),
    paragraphs: vec![
      "ポップカルチャーの記事を読むとき、大切なのは、知らない言葉をすべてすぐに訳すことではありません。まず、文章が何について書かれているのかを考えます。".into(),
      "アニメ、ゲーム、バーチャル配信者についての記事には、作品名や人名が多く出てきます。しかし、固有名詞が分からなくても、筆者が紹介している出来事や意見を追うことはできます。".into(),
      "分からない表現に出会ったら、前後の文を読んでから短いヒントを使いましょう。中国語の訳は、どうしても必要なときだけ開くための安全網です。".into(),
      "毎日一つの記事を最後まで読む経験は、少しずつ文章の流れをつかむ力につながります。今日分からなかった言葉も、別の記事で再会したときに、自分で理解できるようになるかもしれません。".into(),
    ],
    images: vec![],
    reading_minutes: 10,
    difficulty: "N3–N2".into(),
    is_exploration: false,
  }
}

fn article_from_row(row: &rusqlite::Row<'_>) -> Result<Article, rusqlite::Error> {
  let paragraphs_json: String = row.get("paragraphs_json")?;
  let images_json: String = row.get("images_json")?;
  Ok(Article {
    id: row.get("id")?,
    title: row.get("title")?,
    source: row.get("source")?,
    url: row.get("url")?,
    published_at: row.get("published_at")?,
    paragraphs: serde_json::from_str(&paragraphs_json).unwrap_or_default(),
    images: serde_json::from_str(&images_json).unwrap_or_default(),
    reading_minutes: row.get("reading_minutes")?,
    difficulty: row.get("difficulty")?,
    is_exploration: row.get::<_, i64>("is_exploration")? == 1,
  })
}

fn save_article(conn: &Connection, article: &Article) -> Result<(), AppError> {
  conn.execute(
    "INSERT OR REPLACE INTO articles
      (id, day, title, source, url, published_at, paragraphs_json, images_json, reading_minutes, difficulty, is_exploration)
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    params![
      article.id,
      Local::now().format("%Y-%m-%d").to_string(),
      article.title,
      article.source,
      article.url,
      article.published_at,
      serde_json::to_string(&article.paragraphs)
        .map_err(|e| AppError::Message(format!("文章保存失败：{e}")))?,
      serde_json::to_string(&article.images)
        .map_err(|e| AppError::Message(format!("图片保存失败：{e}")))?,
      article.reading_minutes,
      article.difficulty,
      article.is_exploration as i64,
    ],
  )?;
  Ok(())
}

async fn fetch_kaiyou_candidates() -> Result<Vec<TitleCandidate>, AppError> {
  let client = Client::builder()
    .user_agent("NihongoDailyReader/0.1 (personal learning reader)")
    .build()?;
  let home = client.get("https://kai-you.net/").send().await?.text().await?;
  let document = Html::parse_document(&home);
  let link_selector = Selector::parse("a[href^='/article/']")
    .map_err(|_| AppError::Message("无法解析 KAI-YOU 首页".into()))?;
  let mut seen = HashSet::new();
  let candidates = document
    .select(&link_selector)
    .filter_map(|link| {
        let title = link.text().collect::<Vec<_>>().join(" ").trim().to_string();
        let href = link.value().attr("href")?.to_string();
        if title.chars().count() < 12 || !seen.insert(href.clone()) {
          return None;
        }
        Some(TitleCandidate {
          id: href.trim_start_matches('/').replace('/', "-"),
          title,
          url: format!("https://kai-you.net{href}"),
          source: "KAI-YOU".into(),
        })
    })
    .take(12)
    .collect::<Vec<_>>();
  if candidates.is_empty() {
    return Err(AppError::Message("KAI-YOU 首页没有找到可读文章".into()));
  }
  Ok(candidates)
}

async fn fetch_kaiyou_article() -> Result<Article, AppError> {
  let client = Client::builder()
    .user_agent("NihongoDailyReader/0.1 (personal learning reader)")
    .build()?;
  let candidate = fetch_kaiyou_candidates().await?.into_iter().next()
    .ok_or_else(|| AppError::Message("KAI-YOU 首页没有找到可读文章".into()))?;

  let page = client.get(&candidate.url).send().await?.text().await?;
  let article_document = Html::parse_document(&page);
  let paragraph_selector = Selector::parse("article p, main p")
    .map_err(|_| AppError::Message("无法解析文章正文".into()))?;
  let paragraphs = article_document
    .select(&paragraph_selector)
    .map(|p| p.text().collect::<Vec<_>>().join(" ").trim().to_string())
    .filter(|text| text.chars().count() >= 24)
    .take(40)
    .collect::<Vec<_>>();
  let image_selector = Selector::parse("article img, main img")
    .map_err(|_| AppError::Message("无法解析文章图片".into()))?;
  let images = article_document
    .select(&image_selector)
    .filter_map(|image| image.value().attr("src"))
    .filter_map(|src| url::Url::parse(&candidate.url).ok()?.join(src).ok().map(|url| url.to_string()))
    .filter(|src| src.starts_with("https://"))
    .take(6)
    .collect::<Vec<_>>();

  if paragraphs.len() < 3 {
    return Err(AppError::Message("文章正文不足，已跳过该内容".into()));
  }

  Ok(Article {
    id: format!("kaiyou-{}", Uuid::new_v4()),
    title: candidate.title,
    source: "KAI-YOU".into(),
    url: candidate.url,
    published_at: Local::now().format("%Y-%m-%d").to_string(),
    paragraphs,
    images,
    reading_minutes: 10,
    difficulty: "待 AI 评估".into(),
    is_exploration: false,
  })
}

#[tauri::command]
async fn get_title_candidates() -> Result<Vec<TitleCandidate>, AppError> {
  fetch_kaiyou_candidates().await
}

#[tauri::command]
fn save_title_vote(state: State<'_, AppState>, candidate: TitleCandidate, vote: String) -> Result<(), AppError> {
  let conn = open_db(&state)?;
  conn.execute(
    "INSERT INTO title_votes (id, candidate_id, title, url, vote, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    params![Uuid::new_v4().to_string(), candidate.id, candidate.title, candidate.url, vote, Local::now().to_rfc3339()],
  )?;
  Ok(())
}

#[tauri::command]
fn get_initial_assessment() -> Vec<AssessmentQuestion> {
  initial_assessment()
}

#[tauri::command]
fn submit_initial_assessment(state: State<'_, AppState>, answers: Vec<usize>) -> Result<AssessmentResult, AppError> {
  let questions = initial_assessment();
  let score = answers.iter().enumerate().filter(|(index, answer)| questions.get(*index).is_some_and(|question| question.answer_index == **answer)).count();
  let total = questions.len();
  let level_hint = match score {
    0..=3 => "当前建议从 N5–N4 难度起步；AI 会优先提供更短的日语提示。",
    4..=7 => "当前建议从 N4–N3 难度起步；先建立稳定的真实文章阅读习惯。",
    8..=10 => "当前建议从 N3 难度起步；可逐步加入更长的评论与访谈。",
    _ => "当前建议从 N3–N2 难度起步；AI 会更重视语境、篇章推理和自然表达。",
  }.into();
  let conn = open_db(&state)?;
  conn.execute(
    "INSERT INTO assessments (id, mode, score, total, completed_at) VALUES (?1, 'initial', ?2, ?3, ?4)",
    params![Uuid::new_v4().to_string(), score as i64, total as i64, Local::now().to_rfc3339()],
  )?;
  Ok(AssessmentResult { score, total, level_hint })
}

#[tauri::command]
async fn get_today_article(state: State<'_, AppState>) -> Result<Article, AppError> {
  let today = Local::now().format("%Y-%m-%d").to_string();
  {
    let conn = open_db(&state)?;
    conn.execute(
      "UPDATE articles SET paragraphs_json = '[]', images_json = '[]' WHERE day < ?1",
      params![&today],
    )?;
    let existing = conn.query_row(
      "SELECT * FROM articles WHERE day = ?1 ORDER BY rowid DESC LIMIT 1",
      params![&today],
      article_from_row,
    );
    if let Ok(article) = existing {
      return Ok(article);
    }
  }

  let article = fetch_kaiyou_article().await.unwrap_or_else(|_| sample_article());
  let conn = open_db(&state)?;
  let existing = conn.query_row(
    "SELECT * FROM articles WHERE day = ?1 ORDER BY rowid DESC LIMIT 1",
    params![&today],
    article_from_row,
  );
  if let Ok(existing_article) = existing {
    return Ok(existing_article);
  }
  save_article(&conn, &article)?;
  Ok(article)
}

#[tauri::command]
async fn explain_selection(
  state: State<'_, AppState>,
  article_id: String,
  selection: String,
  context: String,
  chinese_revealed: bool,
 ) -> Result<Explanation, AppError> {
  {
    let conn = open_db(&state)?;
    conn.execute(
      "INSERT INTO selections (id, article_id, selection, context, chinese_revealed, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
      params![
        Uuid::new_v4().to_string(),
        article_id,
        selection,
        context,
        chinese_revealed as i64,
        Local::now().to_rfc3339()
      ],
    )?;
  }

  let selection = selection.trim();
  if let Some(api_key) = get_api_key() {
    let system = "你是面向中国母语者的日语阅读教练。用户正在阅读真实日文文章。先提供极短、明显低于原文难度的日语语境提示；中文翻译仅作为用户主动展开后的安全网，但仍必须生成。日语提示中出现汉字时，请在 furigana 字段给出含假名读音的同义提示。不要长篇讲解，不要编造原文没有的信息。";
    let user = format!(
      "选中的文本：{selection}\n完整所在句/上下文：{context}\n用户是否已主动展开中文：{chinese_revealed}"
    );
    return generate_structured(&api_key, system, &user, "selection_explanation", explanation_schema()).await;
  }
  let hint = if selection.chars().count() <= 8 {
    format!("この文では「{}」は、前後の内容を理解するための大切な表現です。", selection)
  } else {
    "選んだ部分が、だれが何をしたか、または筆者の考えを説明しているか確認しましょう。".into()
  };
  Ok(Explanation {
    japanese_hint: hint,
    chinese_translation: "这里应结合前后文理解。配置 OpenAI API Key 后，应用会生成针对该句的中文翻译与语法说明。".into(),
    furigana: "ヒントを読（よ）んでから、必要（ひつよう）なときだけ中国語（ちゅうごくご）を開（ひら）きましょう。".into(),
    note: "本地降级解释：尚未配置 OpenAI API Key。".into(),
  })
}

#[tauri::command]
fn get_ai_status() -> AiStatus {
  AiStatus { configured: get_api_key().is_some(), model: "gpt-5.6-luna".into() }
}

#[tauri::command]
fn save_openai_api_key(api_key: String) -> Result<(), AppError> {
  let cleaned = api_key.trim();
  if cleaned.len() < 20 {
    return Err(AppError::Message("API Key 格式看起来不正确".into()));
  }
  api_key_entry()?
    .set_password(cleaned)
    .map_err(|error| AppError::Message(format!("无法保存到 macOS Keychain：{error}")))
}

#[tauri::command]
async fn get_questions(article: Article) -> Result<Vec<Question>, AppError> {
  if let Some(api_key) = get_api_key() {
    let system = "你是日语阅读测验设计者。根据用户刚读完的真实文章生成恰好三道日语四选一理解题。每一题的正确答案必须仅凭文章得出；evidence 必须逐字引用文章中支持正确答案的一段。错误选项要合理但不能被原文支持。不要问文章外知识，不要剧透，不要使用中文。";
    let user = format!(
      "文章标题：{}\n文章正文：\n{}",
      article.title,
      article.paragraphs.join("\n\n")
    );
    let result: QuestionSet = generate_structured(&api_key, system, &user, "reading_questions", question_schema()).await?;
    if result.questions.iter().all(|question| article.paragraphs.iter().any(|paragraph| paragraph.contains(&question.evidence))) {
      return Ok(result.questions);
    }
    return Err(AppError::Message("AI 题目证据未能在原文中验证，已拒绝使用".into()));
  }
  let evidence = article.paragraphs.first().cloned().unwrap_or_default();
  Ok(vec![Question {
    id: "main-idea".into(),
    prompt: "本文の最初の段落で直接述べられている内容はどれですか。".into(),
    choices: vec![
      evidence.clone(),
      "本文には書かれていない別の出来事を説明している。".into(),
      "本文と関係のない人物について紹介している。".into(),
      "本文にはない結論を先に述べている。".into(),
    ],
    answer_index: 0,
    evidence,
    explanation: "本地保守题：正确选项直接摘自原文首段；配置 API Key 后将生成三道有区分度的证据绑定题。".into(),
  }])
}

#[tauri::command]
fn record_answer(
  state: State<'_, AppState>,
  article_id: String,
  question_id: String,
  chosen_index: usize,
  answer_index: usize,
) -> Result<bool, AppError> {
  let correct = chosen_index == answer_index;
  let conn = open_db(&state)?;
  conn.execute(
    "INSERT INTO answers (id, article_id, question_id, chosen_index, correct, created_at)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    params![
      Uuid::new_v4().to_string(),
      article_id,
      question_id,
      chosen_index as i64,
      correct as i64,
      Local::now().to_rfc3339()
    ],
  )?;
  Ok(correct)
}

#[tauri::command]
fn complete_article(state: State<'_, AppState>, article_id: String) -> Result<(), AppError> {
  let conn = open_db(&state)?;
  conn.execute(
    "UPDATE articles SET completed_at = ?1 WHERE id = ?2",
    params![Local::now().to_rfc3339(), article_id],
  )?;
  Ok(())
}

#[tauri::command]
fn save_topic_feedback(
  state: State<'_, AppState>,
  article_id: String,
  label: String,
) -> Result<(), AppError> {
  let conn = open_db(&state)?;
  conn.execute(
    "INSERT INTO topic_feedback (id, article_id, label, created_at) VALUES (?1, ?2, ?3, ?4)",
    params![Uuid::new_v4().to_string(), article_id, label, Local::now().to_rfc3339()],
  )?;
  Ok(())
}

#[tauri::command]
fn get_progress(state: State<'_, AppState>) -> Result<Progress, AppError> {
  let conn = open_db(&state)?;
  let selected_count = conn.query_row("SELECT COUNT(*) FROM selections", [], |row| row.get(0))?;
  let chinese_reveals = conn.query_row(
    "SELECT COUNT(*) FROM selections WHERE chinese_revealed = 1",
    [],
    |row| row.get(0),
  )?;
  let completed_articles = conn.query_row(
    "SELECT COUNT(*) FROM articles WHERE completed_at IS NOT NULL",
    [],
    |row| row.get(0),
  )?;
  let title_votes = conn.query_row("SELECT COUNT(*) FROM title_votes", [], |row| row.get(0))?;
  let baseline_completed = conn.query_row("SELECT COUNT(*) FROM assessments WHERE mode = 'initial'", [], |row| row.get::<_, i64>(0))? > 0;
  let mut statement = conn.prepare(
    "SELECT label, COUNT(*) FROM topic_feedback GROUP BY label ORDER BY COUNT(*) DESC",
  )?;
  let topic_feedback = statement
    .query_map([], |row| Ok(TopicFeedback { label: row.get(0)?, count: row.get(1)? }))?
    .filter_map(Result::ok)
    .collect();
  Ok(Progress { selected_count, chinese_reveals, completed_articles, title_votes, baseline_completed, topic_feedback })
}

pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    .setup(|app| {
      let data_dir = app.path().app_data_dir().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
      fs::create_dir_all(&data_dir)?;
      app.manage(AppState { db_path: Mutex::new(data_dir.join("learning.sqlite3")) });
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_today_article,
      get_title_candidates,
      save_title_vote,
      get_initial_assessment,
      submit_initial_assessment,
      explain_selection,
      get_ai_status,
      save_openai_api_key,
      get_questions,
      record_answer,
      complete_article,
      save_topic_feedback,
      get_progress
    ])
    .run(tauri::generate_context!())
    .expect("启动日语阅读日报失败");
}
