import { useEffect, useMemo, useRef, useState } from "react";
import { api, type AiStatus, type Article, type AssessmentQuestion, type AssessmentResult, type Explanation, type Progress, type Question, type TitleCandidate } from "./api";

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
  const [showChinese, setShowChinese] = useState(false);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [questionIndex, setQuestionIndex] = useState(0);
  const [answerState, setAnswerState] = useState<{ chosen: number; correct: boolean }>();
  const [completed, setCompleted] = useState(false);
  const [feedback, setFeedback] = useState<string>();
  const [showSettings, setShowSettings] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [keyMessage, setKeyMessage] = useState<string>();
  const [candidates, setCandidates] = useState<TitleCandidate[]>([]);
  const [candidateIndex, setCandidateIndex] = useState(0);
  const [showCalibration, setShowCalibration] = useState(false);
  const [showInitialAssessment, setShowInitialAssessment] = useState(false);
  const [initialQuestions, setInitialQuestions] = useState<AssessmentQuestion[]>([]);
  const [initialIndex, setInitialIndex] = useState(0);
  const [initialAnswers, setInitialAnswers] = useState<number[]>([]);
  const [initialResult, setInitialResult] = useState<AssessmentResult>();
  const readerRef = useRef<HTMLElement>(null);

  const currentQuestion = questions[questionIndex];
  const selectionRateHint = useMemo(() => {
    if (!progress || !article) return "正在建立基线";
    return `${progress.selectedCount} 次划词 · 中文展开 ${progress.chineseReveals} 次`;
  }, [progress, article]);

  useEffect(() => {
    void Promise.all([api.getTodayArticle(), api.getProgress(), api.getAiStatus()])
      .then(([nextArticle, nextProgress, status]) => {
        setArticle(nextArticle);
        setProgress(nextProgress);
        setAiStatus(status);
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
    setShowChinese(false);
    void api.explainSelection(article.id, text, context).then((result) => {
      setExplanation(result);
      void api.getProgress().then(setProgress);
    });
  };

  const revealChinese = () => {
    if (!article || !selection) return;
    setShowChinese(true);
    void api.explainSelection(article.id, selection.text, selection.context, true).then((result) => {
      setExplanation(result);
      void api.getProgress().then(setProgress);
    });
  };

  const finishReading = async () => {
    if (!article || completed) return;
    await api.completeArticle(article.id);
    const generatedQuestions = await api.getQuestions(article);
    setQuestions(generatedQuestions);
    setCompleted(true);
    void api.getProgress().then(setProgress);
  };

  const answerQuestion = async (index: number) => {
    if (!article || !currentQuestion || answerState) return;
    const correct = await api.recordAnswer(article.id, currentQuestion.id, index, currentQuestion.answerIndex);
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
      await api.saveOpenAiApiKey(apiKey);
      const status = await api.getAiStatus();
      setAiStatus(status);
      setApiKey("");
      setKeyMessage("已安全保存到 macOS Keychain。下次划词会使用真实 AI 解释。");
    } catch (reason) {
      setKeyMessage(String(reason));
    }
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
          <span>阅读辅助趋势</span>
          <strong>{selectionRateHint}</strong>
        </div>
        <button className="settings-button" onClick={() => setShowSettings(true)}>AI 设置</button>
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
          <div className="finish-zone">
            <p>已读到文章结尾。</p>
            <button onClick={() => void finishReading()} disabled={completed}>
              {completed ? "已进入理解题" : "完成阅读，开始理解题"}
            </button>
          </div>
        </article>
      </section>

      {selection && (
        <section className="selection-popover" style={{ left: selection.x, top: selection.y }}>
          <button className="close" onClick={() => setSelection(undefined)} aria-label="关闭">×</button>
          <p className="selected">「{selection.text}」</p>
          {explanation ? <>
            <p className="jp-hint">{explanation.japaneseHint}</p>
            <p className="furigana">{explanation.furigana}</p>
            {showChinese ? <p className="chinese">{explanation.chineseTranslation}</p> : <button className="text-button" onClick={revealChinese}>需要时展开中文翻译</button>}
            <small>{explanation.note}</small>
          </> : <p className="thinking">正在生成受控难度的日语提示…</p>}
        </section>
      )}

      {showSettings && <section className="settings-modal">
        <div className="settings-card">
          <button className="close" onClick={() => setShowSettings(false)} aria-label="关闭">×</button>
          <p className="eyebrow">OpenAI Responses API</p>
          <h3>AI 解释设置</h3>
          <p>当前状态：{aiStatus?.configured ? `已配置（${aiStatus.model}）` : "未配置，本地降级解释中"}</p>
          <input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder="粘贴 OpenAI API Key" autoComplete="off" />
          <button className="save-key" onClick={() => void saveKey()} disabled={!apiKey.trim()}>保存到 macOS Keychain</button>
          <small>Key 不会写入文章数据库或 React 前端文件；仅由本机 Rust 进程从 Keychain 读取。</small>
          {keyMessage && <p className="key-message">{keyMessage}</p>}
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
