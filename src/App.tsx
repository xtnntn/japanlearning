import { useEffect, useMemo, useRef, useState } from "react";
import { api, type AbilityProfile, type AiStatus, type Article, type AssessmentQuestion, type AssessmentResult, type Explanation, type Progress, type Question, type ReminderStatus, type TitleCandidate, type WeeklyAssessment } from "./api";

const feedbackOptions = [
  "想多读这个题材",
  "题材还行",
  "题材不感兴趣",
  "不是题材问题，是这篇太难或写得不好"
];

type SelectionState = { text: string; context: string; x: number; y: number };

export default function App() {
  const [article, setArticle] = useState<Article>();
  const [progress, setProgress] = useState<Progress>();
  const [aiStatus, setAiStatus] = useState<AiStatus>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [selection, setSelection] = useState<SelectionState>();
  const [explanation, setExplanation] = useState<Explanation>();
  const [explanationError, setExplanationError] = useState<string>();
  const [questions, setQuestions] = useState<Question[]>([]);
  const [questionIndex, setQuestionIndex] = useState(0);
  const [answerState, setAnswerState] = useState<{ chosen: number; correct: boolean }>();
  const [completed, setCompleted] = useState(false);
  const [quizError, setQuizError] = useState<string>();
  const [feedback, setFeedback] = useState<string>();
  const [showSettings, setShowSettings] = useState(false);
  const [showProgress, setShowProgress] = useState(false);
  const [showProfile, setShowProfile] = useState(false);
  const [abilityProfile, setAbilityProfile] = useState<AbilityProfile>();
  const [targetLevel, setTargetLevel] = useState("");
  const [profileMessage, setProfileMessage] = useState<string>();
  const [reminder, setReminder] = useState<ReminderStatus>({ enabled: false, hour: 9, minute: 0 });
  const [reminderTime, setReminderTime] = useState("09:00");
  const [reminderMessage, setReminderMessage] = useState<string>();
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("https://api.openai.com/v1");
  const [model, setModel] = useState("gpt-5.6-luna");
  const [protocol, setProtocol] = useState<"responses" | "chat_completions">("responses");
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [detectingModels, setDetectingModels] = useState(false);
  const [keyMessage, setKeyMessage] = useState<string>();
  const [candidates, setCandidates] = useState<TitleCandidate[]>([]);
  const [candidateIndex, setCandidateIndex] = useState(0);
  const [showCalibration, setShowCalibration] = useState(false);
  const [showInitialAssessment, setShowInitialAssessment] = useState(false);
  const [initialQuestions, setInitialQuestions] = useState<AssessmentQuestion[]>([]);
  const [initialIndex, setInitialIndex] = useState(0);
  const [initialAnswers, setInitialAnswers] = useState<number[]>([]);
  const [initialResult, setInitialResult] = useState<AssessmentResult>();
  const [weeklyAssessment, setWeeklyAssessment] = useState<WeeklyAssessment>();
  const [showWeeklyAssessment, setShowWeeklyAssessment] = useState(false);
  const [weeklyQuestionIndex, setWeeklyQuestionIndex] = useState(-1);
  const [weeklyAnswers, setWeeklyAnswers] = useState<number[]>([]);
  const [weeklyResult, setWeeklyResult] = useState<AssessmentResult>();
  const readerRef = useRef<HTMLElement>(null);
  const selectionPopoverRef = useRef<HTMLElement>(null);
  const finishRef = useRef<HTMLDivElement>(null);
  const generatingQuestionsRef = useRef(false);

  const currentQuestion = questions[questionIndex];
  const selectionRateHint = useMemo(() => {
    if (!progress || !article) return "正在建立基线";
    return `${progress.selectedCount} 次划词 · 中文展开 ${progress.chineseReveals} 次`;
  }, [progress, article]);

  useEffect(() => {
    void Promise.all([api.getTodayArticle(), api.getProgress(), api.getAiStatus(), api.getReminderStatus()])
      .then(([nextArticle, nextProgress, status, reminderStatus]) => {
        setArticle(nextArticle);
        setProgress(nextProgress);
        setAiStatus(status);
        setBaseUrl(status.baseUrl);
        setModel(status.model);
        setProtocol(status.protocol);
        setAvailableModels([status.model]);
        setReminder(reminderStatus);
        setReminderTime(`${String(reminderStatus.hour).padStart(2, "0")}:${String(reminderStatus.minute).padStart(2, "0")}`);
        if (nextProgress.titleVotes === 0) {
          void api.getTitleCandidates().then((items) => {
            setCandidates(items);
            setShowCalibration(items.length > 0);
          });
        } else if (!nextProgress.baselineCompleted) {
          void api.getInitialAssessment().then((items) => {
            setInitialQuestions(items);
            setShowInitialAssessment(items.length > 0);
          });
        }
      })
      .catch((reason: unknown) => setError(String(reason)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    if (!selection) return;
    const closeWhenClickingOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!selectionPopoverRef.current?.contains(target)) {
        setSelection(undefined);
      }
    };
    window.addEventListener("pointerdown", closeWhenClickingOutside);
    return () => window.removeEventListener("pointerdown", closeWhenClickingOutside);
  }, [selection]);

  const handleSelection = () => {
    const nativeSelection = window.getSelection();
    const text = nativeSelection?.toString().trim() ?? "";
    const range = nativeSelection?.rangeCount ? nativeSelection.getRangeAt(0) : undefined;
    if (!article || !text || !range || !readerRef.current?.contains(range.commonAncestorContainer)) return;
    if (text.length > 160) return;
    const rect = range.getBoundingClientRect();
    const context = range.commonAncestorContainer.parentElement?.closest("p")?.textContent ?? text;
    setSelection({ text, context, x: rect.left + rect.width / 2, y: rect.bottom + 12 });
    setExplanation(undefined);
    setExplanationError(undefined);
    void api.explainSelection(article.id, text, context)
      .then((result) => { setExplanation(result); void api.getProgress().then(setProgress); })
      .catch((reason) => setExplanationError(String(reason)));
  };

  const finishReading = async () => {
    if (!article || completed || generatingQuestionsRef.current) return;
    generatingQuestionsRef.current = true;
    setQuizError(undefined);
    try {
      await api.completeArticle(article.id);
      const generatedQuestions = await api.getQuestions(article);
      setQuestions(generatedQuestions);
      setCompleted(true);
      void api.getProgress().then(setProgress);
    } catch (reason) {
      setQuizError(String(reason));
    } finally {
      generatingQuestionsRef.current = false;
    }
  };

  useEffect(() => {
    const target = finishRef.current;
    if (!target || completed) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void finishReading();
    }, { threshold: 0.75 });
    observer.observe(target);
    return () => observer.disconnect();
  }, [article?.id, completed]);

  const answerQuestion = async (index: number) => {
    if (!article || !currentQuestion || answerState) return;
    const correct = await api.recordAnswer(article.id, currentQuestion.id, index, currentQuestion.answerIndex, currentQuestion.testedExpressions);
    setAnswerState({ chosen: index, correct });
  };

  const nextQuestion = () => {
    if (questionIndex + 1 < questions.length) {
      setQuestionIndex((index) => index + 1);
      setAnswerState(undefined);
    }
  };

  const submitFeedback = async (label: string) => {
    if (!article) return;
    await api.saveTopicFeedback(article.id, label);
    setFeedback(label);
    void api.getProgress().then(setProgress);
  };

  const saveKey = async () => {
    try {
      await api.saveOpenAiApiKey(apiKey, baseUrl, model, protocol);
      const status = await api.getAiStatus();
      setAiStatus(status);
      setApiKey("");
      setBaseUrl(status.baseUrl);
      setModel(status.model);
      setProtocol(status.protocol);
      setKeyMessage("API Key 已保存到 macOS Keychain；Base URL、协议和模型已保存到本地设置。下次划词会使用所选配置。");
    } catch (reason) {
      setKeyMessage(String(reason));
    }
  };

  const detectModels = async () => {
    setDetectingModels(true);
    setKeyMessage(undefined);
    try {
      const discovered = await api.discoverModels(baseUrl, apiKey);
      setAvailableModels(discovered);
      setModel(discovered.includes(model) ? model : discovered[0]);
      setKeyMessage(`已检测到 ${discovered.length} 个模型，请选择一个后保存。`);
    } catch (reason) {
      setAvailableModels([]);
      setKeyMessage(String(reason));
    } finally {
      setDetectingModels(false);
    }
  };

  const saveReminder = async () => {
    const [hour, minute] = reminderTime.split(":").map(Number);
    try {
      const status = await api.installDailyReminder(hour, minute);
      setReminder(status);
      setReminderMessage(`已启用，每天 ${reminderTime} 提醒。`);
    } catch (reason) { setReminderMessage(String(reason)); }
  };

  const disableReminder = async () => {
    try {
      setReminder(await api.removeDailyReminder());
      setReminderMessage("每日提醒已关闭。");
    } catch (reason) { setReminderMessage(String(reason)); }
  };

  const openProfile = async () => {
    try {
      const profile = await api.getAbilityProfile();
      setAbilityProfile(profile);
      setTargetLevel(profile.targetLevel ?? "");
      setProfileMessage(undefined);
      setShowProfile(true);
    } catch (reason) { setError(String(reason)); }
  };

  const saveTargetLevel = async () => {
    try {
      const profile = await api.updateTargetLevel(targetLevel || undefined);
      setAbilityProfile(profile);
      setTargetLevel(profile.targetLevel ?? "");
      setProfileMessage("目标难度已保存，将从下一篇文章开始生效。");
    } catch (reason) { setProfileMessage(String(reason)); }
  };

  const voteForTitle = async (vote: string) => {
    const candidate = candidates[candidateIndex];
    if (!candidate) return;
    await api.saveTitleVote(candidate, vote);
    const nextIndex = candidateIndex + 1;
    if (nextIndex >= Math.min(candidates.length, 8)) {
      setShowCalibration(false);
      void api.getProgress().then(setProgress);
      void api.getInitialAssessment().then((items) => {
        setInitialQuestions(items);
        setShowInitialAssessment(items.length > 0);
      });
      return;
    }
    setCandidateIndex(nextIndex);
  };

  const answerInitialQuestion = async (answer: number) => {
    const answers = [...initialAnswers, answer];
    setInitialAnswers(answers);
    if (initialIndex + 1 < initialQuestions.length) {
      setInitialIndex((index) => index + 1);
      return;
    }
    const result = await api.submitInitialAssessment(answers);
    setInitialResult(result);
    void api.getProgress().then(setProgress);
  };

  const openWeeklyAssessment = async () => {
    try {
      const assessment = await api.getWeeklyAssessment();
      setWeeklyAssessment(assessment);
      setWeeklyResult(assessment.result);
      setWeeklyQuestionIndex(assessment.completed ? assessment.questions.length : -1);
      setWeeklyAnswers([]);
      setShowWeeklyAssessment(true);
    } catch (reason) {
      setError(String(reason));
    }
  };

  const startWeeklyQuestions = () => setWeeklyQuestionIndex(0);

  const answerWeeklyQuestion = async (answer: number) => {
    if (!weeklyAssessment || weeklyQuestionIndex < 0) return;
    const answers = [...weeklyAnswers, answer];
    setWeeklyAnswers(answers);
    if (weeklyQuestionIndex + 1 < weeklyAssessment.questions.length) {
      setWeeklyQuestionIndex((index) => index + 1);
      return;
    }
    const result = await api.submitWeeklyAssessment(weeklyAssessment.id, answers);
    setWeeklyResult(result);
    setWeeklyAssessment({ ...weeklyAssessment, completed: true, result });
    void api.getProgress().then(setProgress);
  };

  if (loading) return <main className="loading">正在准备今天唯一的一篇阅读…</main>;
  if (error || !article) return <main className="loading error">启动失败：{error ?? "没有可用文章"}</main>;

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">日语阅读日报 · TODAY</p>
          <h1>今天只读这一篇</h1>
        </div>
        <div className="metric-card">
          <span>旧表达独立理解率</span>
          <strong>{progress?.independentExpressionRate == null ? "正在建立证据" : `${Math.round(progress.independentExpressionRate * 100)}%`}</strong>
          <small>{progress?.independentExpressionAttempts ?? 0} 次新语境验证</small>
          <small>{selectionRateHint}</small>
          <small>{progress?.completedArticles ?? 0} 篇完成 · {progress?.missedArticles ?? 0} 篇过期</small>
        </div>
        <button className="settings-button" onClick={() => {
          setBaseUrl(aiStatus?.baseUrl ?? "https://api.openai.com/v1");
          setModel(aiStatus?.model ?? "gpt-5.6-luna");
          setProtocol(aiStatus?.protocol ?? "responses");
          setShowSettings(true);
        }}>AI 设置</button>
        <button className="settings-button" onClick={() => void openWeeklyAssessment()}>本周独立评估</button>
        <button className="settings-button" onClick={() => setShowProgress(true)}>进步曲线</button>
        <button className="settings-button" onClick={() => void openProfile()}>能力画像</button>
      </header>

      <section className="article-layout">
        <aside className="article-meta">
          <span className="source">{article.source}</span>
          <span>{article.publishedAt}</span>
          <span>{article.readingMinutes} 分钟</span>
          <span>{article.difficulty}</span>
          {article.isExploration && <span className="explore">探索题材</span>}
          <a href={article.url} target="_blank" rel="noreferrer">查看原始来源 ↗</a>
        </aside>

        <article className="reader" ref={readerRef} onMouseUp={handleSelection}>
          <h2>{article.title}</h2>
          <p className="reader-tip">任意划选词、短语或句子。先读日语提示；需要时再展开中文。</p>
          {article.paragraphs.map((paragraph, index) => <section className="article-block" key={index}>
            <p>{paragraph}</p>
            {index < article.images.length && <a className="article-image-link" href={article.url} target="_blank" rel="noreferrer"><img src={article.images[index]} alt="来源文章配图" referrerPolicy="no-referrer" /></a>}
          </section>)}
          {article.embeds.length > 0 && <section className="article-embeds">
            {article.embeds.map((embed) => embed.kind === "video" && embed.url.includes("youtube.com/embed/")
              ? <iframe key={embed.url} src={embed.url} title="来源文章视频" loading="lazy" allowFullScreen />
              : <a key={embed.url} href={embed.url} target="_blank" rel="noreferrer">查看来源文章中的社媒或视频内容 ↗</a>)}
          </section>}
          <div className="finish-zone" ref={finishRef}>
            <p>{completed ? "已到达文末，理解题已出现。" : quizError ? `理解题准备失败：${quizError}` : "到达这里会自动准备理解题。"}</p>
            <button onClick={() => void finishReading()} disabled={completed}>
              {completed ? "已进入理解题" : "没有自动出现？点击重试"}
            </button>
          </div>
        </article>
      </section>

      {selection && (
        <section ref={selectionPopoverRef} className="selection-popover" style={{ left: selection.x, top: selection.y }}>
          <p className="selected">「{selection.text}」</p>
          {explanation ? <>
            <p className="reading"><strong>读音</strong> {explanation.reading}</p>
            <p className="translation"><strong>译文</strong> {explanation.translation}</p>
            <p><strong>语境</strong> {explanation.contextNote}</p>
            <div className="example"><strong>例句</strong><p>{explanation.example}</p><p>{explanation.exampleTranslation}</p></div>
            <p className="grammar"><strong>语法/搭配</strong> {explanation.grammarNote}</p>
          </> : explanationError ? <p className="inline-error">生成失败：{explanationError}<br />请关闭后重新划选一次。</p> : <p className="thinking">正在结合上下文生成解释…</p>}
        </section>
      )}

      {showSettings && <section className="settings-modal">
        <div className="settings-card ai-settings-card">
          <button className="close" onClick={() => setShowSettings(false)} aria-label="关闭">×</button>
          <p className="eyebrow">OpenAI 兼容 API</p>
          <h3>AI 解释设置</h3>
          <p>当前状态：{aiStatus?.configured ? `已配置（${aiStatus.model} · ${aiStatus.protocol === "chat_completions" ? "Chat Completions" : "Responses"}）` : "未配置，本地降级解释中"}</p>
          <div className="ai-config-grid">
            <div className="wide-field"><label className="field-label" htmlFor="base-url">Base URL</label><input id="base-url" type="url" value={baseUrl} onChange={(event) => { setBaseUrl(event.target.value); setAvailableModels([]); }} placeholder="https://api.openai.com/v1" autoComplete="url" /></div>
            <div><label className="field-label" htmlFor="protocol">调用协议</label><select id="protocol" value={protocol} onChange={(event) => setProtocol(event.target.value as "responses" | "chat_completions")}><option value="responses">Responses</option><option value="chat_completions">Chat Completions</option></select></div>
            <div><label className="field-label" htmlFor="model">模型</label><select id="model" value={model} onChange={(event) => setModel(event.target.value)} disabled={availableModels.length === 0}>{availableModels.length === 0 ? <option>请先检测可用模型</option> : availableModels.map((name) => <option key={name} value={name}>{name}</option>)}</select></div>
            <div className="wide-field"><label className="field-label" htmlFor="api-key">API Key</label><input id="api-key" type="password" value={apiKey} onChange={(event) => { setApiKey(event.target.value); setAvailableModels([]); }} placeholder={aiStatus?.configured ? "已安全保存；仅在替换 Key 时输入" : "粘贴 API Key"} autoComplete="off" /></div>
          </div>
          <button className="detect-models" onClick={() => void detectModels()} disabled={!baseUrl.trim() || (!apiKey.trim() && !aiStatus?.configured) || detectingModels}>{detectingModels ? "正在检测模型…" : "检测可用模型"}</button>
          <button className="save-key" onClick={() => void saveKey()} disabled={!baseUrl.trim() || availableModels.length === 0 || (!apiKey.trim() && !aiStatus?.configured)}>保存 AI 设置</button>
          <small>只有 API Key 会访问 macOS Keychain；Base URL、协议和模型保存在本地应用设置，不会触发钥匙串授权。Responses 调用 <code>/responses</code>；Chat Completions 调用 <code>/chat/completions</code>。你的 tokendance 网关应选择后者。</small>
          {keyMessage && <p className="key-message">{keyMessage}</p>}
          <div className="settings-divider" />
          <p className="eyebrow">每日阅读提醒</p>
          <p>当前状态：{reminder.enabled ? `已启用（${String(reminder.hour).padStart(2, "0")}:${String(reminder.minute).padStart(2, "0")}）` : "未启用"}</p>
          <label className="field-label" htmlFor="reminder-time">提醒时间</label>
          <input id="reminder-time" type="time" value={reminderTime} onChange={(event) => setReminderTime(event.target.value)} />
          <div className="reminder-actions">
            {reminder.enabled && <button className="secondary-action" onClick={() => void disableReminder()}>关闭提醒</button>}
            <button className="save-key" onClick={() => void saveReminder()}>{reminder.enabled ? "更新时间" : "启用提醒"}</button>
          </div>
          {reminderMessage && <p className="key-message">{reminderMessage}</p>}
        </div>
      </section>}

      {showProgress && <section className="settings-modal">
        <div className="settings-card progress-card">
          <button className="close" onClick={() => setShowProgress(false)} aria-label="关闭">×</button>
          <p className="eyebrow">最近 14 天</p>
          <h3>阅读辅助依赖</h3>
          <div className={`experiment-status ${progress?.experiment.readyForVerdict ? "ready" : "collecting"}`}>
            <div><span>两周试验</span><strong>{progress?.experiment.verdict ?? "证据积累中"}</strong></div>
            <small>观察第 {progress?.experiment.observedDays ?? 0} 天 · 有效阅读 {progress?.experiment.completedDays ?? 0} 天</small>
            <ul>
              <li>归一化划词频率：{progress?.experiment.selectionRateChange == null ? "至少需要 6 篇完成文章" : progress.experiment.selectionRateChange < 0 ? "下降中" : "尚未下降"}</li>
              <li>无辅助周测：{progress?.experiment.weeklyScoreNonDeclining == null ? "至少需要 2 次周测" : progress.experiment.weeklyScoreNonDeclining ? "未下降" : "出现下降"}</li>
              <li>旧表达独立理解：{progress?.experiment.expressionRateChange == null ? "前后各需至少 4 次验证" : progress.experiment.expressionRateChange > 0 ? "上升中" : "尚未上升"}</li>
            </ul>
          </div>
          <p>每千字划词次数已按文章难度校正。曲线向下，且独立评估不下降，才说明阅读正在变轻松。</p>
          <div className="primary-progress-metric">
            <span>新语境旧表达独立理解率</span>
            <strong>{progress?.independentExpressionRate == null ? "证据不足" : `${Math.round(progress.independentExpressionRate * 100)}%`}</strong>
            <small>仅统计历史划词在新文章再次出现、对应理解题已作答的 {progress?.independentExpressionAttempts ?? 0} 次证据；当天再次划词不计独立理解。</small>
          </div>
          {progress?.selectionTrend.length ? <div className="trend-chart" aria-label="归一化划词频率曲线">
            {progress.selectionTrend.map((point, index) => {
              const max = Math.max(...progress.selectionTrend.map((item) => item.normalizedRate), 1);
              return <div className="trend-column" key={`${point.day}-${index}`} title={`${point.day}：${point.normalizedRate.toFixed(1)} 次/千字`}>
                <span>{point.normalizedRate.toFixed(1)}</span>
                <i style={{ height: `${Math.max(5, point.normalizedRate / max * 120)}px` }} />
                <small>{point.day.slice(5)}</small>
              </div>;
            })}
          </div> : <p className="empty-trend">完成阅读并划词后，这里会形成趋势。</p>}
          <h3 className="trend-subtitle">独立阅读理解</h3>
          {progress?.assessmentTrend.length ? <div className="assessment-history">{progress.assessmentTrend.map((point) => <div key={point.week}><span>{point.week}</span><strong>{Math.round(point.scoreRate * 100)}%</strong></div>)}</div> : <p className="empty-trend">完成本周独立评估后开始记录。</p>}
        </div>
      </section>}

      {showProfile && abilityProfile && <section className="settings-modal">
        <div className="settings-card profile-card">
          <button className="close" onClick={() => setShowProfile(false)} aria-label="关闭">×</button>
          <p className="eyebrow">可查看 · 可校正</p>
          <h3>基础能力画像</h3>
          <p>这是学习证据的汇总，不是永久 JLPT 标签。人工设置只控制未来选文目标难度。</p>
          <div className="profile-grid">
            <div><span>系统建议</span><strong>{abilityProfile.suggestedLevel}</strong></div>
            <div><span>完成文章</span><strong>{abilityProfile.completedArticles}</strong></div>
            <div><span>初始定位</span><strong>{abilityProfile.initialScore == null ? "待测" : `${Math.round(abilityProfile.initialScore * 100)}%`}</strong></div>
            <div><span>日常理解题</span><strong>{abilityProfile.dailyAccuracy == null ? "证据不足" : `${Math.round(abilityProfile.dailyAccuracy * 100)}%`}</strong></div>
            <div><span>独立周测</span><strong>{abilityProfile.weeklyAccuracy == null ? "证据不足" : `${Math.round(abilityProfile.weeklyAccuracy * 100)}%`}</strong></div>
            <div><span>中文展开率</span><strong>{abilityProfile.chineseRevealRate == null ? "证据不足" : `${Math.round(abilityProfile.chineseRevealRate * 100)}%`}</strong></div>
          </div>
          <label className="field-label" htmlFor="target-level">未来文章目标难度</label>
          <select id="target-level" value={targetLevel} onChange={(event) => setTargetLevel(event.target.value)}>
            <option value="">自动判断</option>
            {["N5", "N4", "N3", "N2", "N1"].map((level) => <option key={level} value={level}>{level}</option>)}
          </select>
          <button className="save-key" onClick={() => void saveTargetLevel()}>保存校正</button>
          {profileMessage && <p className="key-message">{profileMessage}</p>}
        </div>
      </section>}

      {showWeeklyAssessment && weeklyAssessment && <section className="settings-modal">
        <div className="settings-card weekly-card">
          <button className="close" onClick={() => setShowWeeklyAssessment(false)} aria-label="关闭">×</button>
          <p className="eyebrow">每周独立评估 · {weeklyAssessment.week}</p>
          <h3>不使用翻译，读完这篇新文章</h3>
          <p className="weekly-rule">评估期间不提供划词解释或中文翻译。结果与日常辅助阅读分开记录。</p>
          {weeklyResult ? <div className="assessment-result">
            <strong>{weeklyResult.score} / {weeklyResult.total}</strong>
            <p>{weeklyResult.levelHint}</p>
          </div> : weeklyQuestionIndex < 0 ? <>
            <p className="weekly-title">{weeklyAssessment.article.title}</p>
            <div className="weekly-reader">{weeklyAssessment.article.paragraphs.map((paragraph, index) => <p key={index}>{paragraph}</p>)}</div>
            <button className="save-key" onClick={startWeeklyQuestions}>我已读完，开始理解题</button>
          </> : <>
            <p className="assessment-prompt">{weeklyAssessment.questions[weeklyQuestionIndex]?.prompt}</p>
            <div className="assessment-options">{weeklyAssessment.questions[weeklyQuestionIndex]?.choices.map((choice, index) => <button key={`${weeklyQuestionIndex}-${choice}`} onClick={() => void answerWeeklyQuestion(index)}>{choice}</button>)}</div>
            <small>{weeklyQuestionIndex + 1} / {weeklyAssessment.questions.length}</small>
          </>}
        </div>
      </section>}

      {showCalibration && candidates[candidateIndex] && <section className="settings-modal">
        <div className="settings-card calibration-card">
          <p className="eyebrow">兴趣冷启动 {candidateIndex + 1} / {Math.min(candidates.length, 8)}</p>
          <h3>这个标题会想读吗？</h3>
          <p className="candidate-title">{candidates[candidateIndex].title}</p>
          <small>AI 会用这些快速反馈挑选今后的文章；真实阅读、划词和测验的权重会更高。</small>
          <div className="calibration-actions">
            <button onClick={() => void voteForTitle("不想看")}>不想看</button>
            <button onClick={() => void voteForTitle("无感")}>无感</button>
            <button className="interested" onClick={() => void voteForTitle("想看")}>想看</button>
          </div>
        </div>
      </section>}

      {showInitialAssessment && initialQuestions[initialIndex] && <section className="settings-modal">
        <div className="settings-card calibration-card assessment-card">
          {!initialResult ? <>
            <p className="eyebrow">初始定位 {initialIndex + 1} / {initialQuestions.length}</p>
            <h3>先建立你的阅读起点</h3>
            <p className="assessment-prompt">{initialQuestions[initialIndex].prompt}</p>
            <div className="assessment-options">{initialQuestions[initialIndex].choices.map((choice, index) => <button key={choice} onClick={() => void answerInitialQuestion(index)}>{choice}</button>)}</div>
            <small>这不是永久等级标签。今后的划词、真实阅读和每周独立评估会持续修正它。</small>
          </> : <>
            <p className="eyebrow">定位完成</p>
            <h3>{initialResult.score} / {initialResult.total}</h3>
            <p>{initialResult.levelHint}</p>
            <button className="save-key" onClick={() => setShowInitialAssessment(false)}>开始今天的阅读</button>
          </>}
        </div>
      </section>}

      {completed && currentQuestion && (
        <section className="overlay-panel quiz-panel">
          <p className="eyebrow">理解题 {questionIndex + 1} / {questions.length}</p>
          <h3>{currentQuestion.prompt}</h3>
          <div className="choices">
            {currentQuestion.choices.map((choice, index) => {
              const isChosen = answerState?.chosen === index;
              const isAnswer = index === currentQuestion.answerIndex;
              return <button key={choice} disabled={Boolean(answerState)} onClick={() => void answerQuestion(index)} className={answerState ? (isAnswer ? "correct" : isChosen ? "wrong" : "") : ""}>{choice}</button>;
            })}
          </div>
          {answerState && <div className="evidence">
            <strong>{answerState.correct ? "答对了" : "再看一次原文证据"}</strong>
            <blockquote>{currentQuestion.evidence}</blockquote>
            <p>{currentQuestion.explanation}</p>
            {questionIndex + 1 < questions.length ? <button onClick={nextQuestion}>下一题</button> : <p className="done">理解题完成。请选择对今天题材的感觉。</p>}
          </div>}
        </section>
      )}

      {completed && !currentQuestion && <section className="overlay-panel quiz-panel"><p>正在准备理解题…</p></section>}

      {completed && questions.length > 0 && questionIndex === questions.length - 1 && answerState && (
        <section className="feedback-panel">
          <p className="eyebrow">题材反馈</p>
          <h3>你对今天这个题材的感觉？</h3>
          {feedback ? <p className="feedback-confirmed">已记录：{feedback}</p> : <div className="feedback-options">{feedbackOptions.map((option) => <button key={option} onClick={() => void submitFeedback(option)}>{option}</button>)}</div>}
        </section>
      )}
    </main>
  );
}
