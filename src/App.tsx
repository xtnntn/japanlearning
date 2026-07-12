import React, { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, type AbilityProfile, type AiStatus, type Article, type AssessmentQuestion, type AssessmentResult, type Explanation, type MultiAgentPlan, type Progress, type Question, type ReminderStatus, type ReviewCard, type TitleCandidate, type WeeklyAssessment } from "./api";

const feedbackOptions = [
  "想多读这个题材",
  "题材还行",
  "题材不感兴趣",
  "不是题材问题，是这篇太难或写得不好"
];

type SelectionState = { text: string; context: string; x: number; y: number; placement: "above" | "below"; };

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

function Button({
  className = "",
  variant = "secondary",
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  return (
    <button className={`btn btn-${variant} ${className}`.trim()} {...props}>
      {children}
    </button>
  );
}

type ModalSize = "sm" | "md" | "lg" | "xl" | "full";

function Modal({
  open,
  onClose,
  title,
  titleId,
  children,
  size = "md",
  className = "",
}: {
  open: boolean;
  onClose: () => void;
  title?: string;
  titleId?: string;
  children: ReactNode;
  size?: ModalSize;
  className?: string;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (open) {
      previousFocusRef.current = document.activeElement as HTMLElement;
      document.body.style.overflow = "hidden";
      const timer = window.setTimeout(() => {
        const firstFocusable = cardRef.current?.querySelector<HTMLElement>(
          "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])"
        );
        firstFocusable?.focus();
      }, 0);
      return () => {
        window.clearTimeout(timer);
      };
    }
    document.body.style.overflow = "";
    if (previousFocusRef.current && typeof previousFocusRef.current.focus === "function") {
      previousFocusRef.current.focus();
      previousFocusRef.current = null;
    }
    return () => {};
  }, [open]);

  const onCloseRef = useRef(onClose);
  useEffect(() => { onCloseRef.current = onClose; }, [onClose]);

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab" || !cardRef.current) return;
      const focusable = Array.from(
        cardRef.current.querySelectorAll<HTMLElement>(
          "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])"
        )
      ).filter((el) => !el.hasAttribute("disabled") && el.offsetParent !== null);
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open]);

  if (!open) return null;

  const labelledBy = titleId ?? (title ? "modal-title" : undefined);

  return (
    <section
      className="modal-overlay"
      onClick={(event) => {
        if (event.currentTarget === event.target) onClose();
      }}
    >
      <div
        ref={cardRef}
        className={`modal-card size-${size} ${className}`.trim()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
      >
        <button
          className="modal-close"
          onClick={onClose}
          aria-label="关闭"
          type="button"
        >
          ×
        </button>
        {title && !titleId && (
          <h3 id="modal-title">{title}</h3>
        )}
        {children}
      </div>
    </section>
  );
}

function MenuIcon({ name }: { name: "ai" | "weekly" | "deck" | "progress" | "profile" | "refresh" }) {
  const icons: Record<string, ReactNode> = {
    ai: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 2l2.4 7.2h7.6l-6 4.8 2.4 7.2-6-4.8-6 4.8 2.4-7.2-6-4.8h7.6z" />
      </svg>
    ),
    weekly: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="M8 2v4M16 2v4M3 10h18" />
      </svg>
    ),
    deck: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <rect x="5" y="6" width="14" height="12" rx="2" />
        <path d="M8 10h8M8 14h5" />
      </svg>
    ),
    progress: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M3 17v4M9 13v8M15 9v12M21 5v16" />
      </svg>
    ),
    profile: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="8" r="4" />
        <path d="M4 20c0-4 4-6 8-6s8 2 8 6" />
      </svg>
    ),
    refresh: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M21 12a9 9 0 1 1-2.64-6.36" />
        <path d="M21 3v9h-9" />
      </svg>
    ),
  };
  return icons[name];
}

export default function App() {
  const [article, setArticle] = useState<Article>();
  const [progress, setProgress] = useState<Progress>();
  const [aiStatus, setAiStatus] = useState<AiStatus>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [refreshingDaily, setRefreshingDaily] = useState(false);
  const [dailyRefreshMessage, setDailyRefreshMessage] = useState<string>();
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
  const [menuCollapsed, setMenuCollapsed] = useState(false);
  const [showReviewDeck, setShowReviewDeck] = useState(false);
  const [reviewCards, setReviewCards] = useState<ReviewCard[]>([]);
  const [deckIndex, setDeckIndex] = useState(0);
  const [showProfile, setShowProfile] = useState(false);
  const [abilityProfile, setAbilityProfile] = useState<AbilityProfile>();
  const [multiAgentPlan, setMultiAgentPlan] = useState<MultiAgentPlan | null>();
  const [refreshingAgentPlan, setRefreshingAgentPlan] = useState(false);
  const [targetLevel, setTargetLevel] = useState("");
  const [profileMessage, setProfileMessage] = useState<{ text: string; type: "success" | "error" }>();
  const [reminder, setReminder] = useState<ReminderStatus>({ enabled: false, hour: 9, minute: 0 });
  const [reminderTime, setReminderTime] = useState("09:00");
  const [reminderMessage, setReminderMessage] = useState<{ text: string; type: "success" | "error" }>();
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("https://api.openai.com/v1");
  const [model, setModel] = useState("gpt-5.6-luna");
  const [protocol, setProtocol] = useState<"responses" | "chat_completions">("responses");
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [detectingModels, setDetectingModels] = useState(false);
  const [keyMessage, setKeyMessage] = useState<{ text: string; type: "success" | "error" | "info" }>();
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
  const [reviewCard, setReviewCard] = useState<ReviewCard | null>();
  const [reviewRevealed, setReviewRevealed] = useState(false);
  const [reviewingCard, setReviewingCard] = useState(false);
  const [inlineReviewDismissed, setInlineReviewDismissed] = useState(false);
  const readerRef = useRef<HTMLElement>(null);
  const selectionPopoverRef = useRef<HTMLElement>(null);
  const finishRef = useRef<HTMLDivElement>(null);
  const generatingQuestionsRef = useRef(false);
  const selectionRequestTimerRef = useRef<number | undefined>(undefined);
  const selectionRequestSequenceRef = useRef(0);

  const currentQuestion = questions[questionIndex];
  const deckCard = reviewCards[deckIndex];
  const selectionRateHint = useMemo(() => {
    if (!progress || !article) return "正在建立基线";
    return `标记 ${progress.selectedCount} 处 · 中文辅助 ${progress.chineseReveals} 次`;
  }, [progress, article]);

  useEffect(() => {
    void Promise.all([api.getTodayArticle(), api.getProgress(), api.getAiStatus(), api.getReminderStatus(), api.getDueReviewCard()])
      .then(([nextArticle, nextProgress, status, reminderStatus, dueCard]) => {
        setArticle(nextArticle);
        setProgress(nextProgress);
        setAiStatus(status);
        setBaseUrl(status.baseUrl);
        setModel(status.model);
        setProtocol(status.protocol);
        setAvailableModels([status.model]);
        setReminder(reminderStatus);
        setReviewCard(dueCard);
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
    const dismissSelection = () => {
      selectionRequestSequenceRef.current += 1;
      window.clearTimeout(selectionRequestTimerRef.current);
      setSelection(undefined);
      setExplanation(undefined);
      setExplanationError(undefined);
    };
    const closeWhenClickingOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!selectionPopoverRef.current?.contains(target)) {
        dismissSelection();
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") dismissSelection();
    };
    window.addEventListener("pointerdown", closeWhenClickingOutside);
    window.addEventListener("scroll", dismissSelection, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeWhenClickingOutside);
      window.removeEventListener("scroll", dismissSelection, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [selection]);

  useEffect(() => () => window.clearTimeout(selectionRequestTimerRef.current), []);

  const handleSelection = () => {
    const nativeSelection = window.getSelection();
    const text = nativeSelection?.toString().trim() ?? "";
    const range = nativeSelection?.rangeCount ? nativeSelection.getRangeAt(0) : undefined;
    if (!article || !text || !range || !readerRef.current?.contains(range.commonAncestorContainer)) return;
    if (text.length > 160) {
      setExplanationError("划选内容过长，请控制在 160 字以内。");
      return;
    }
    const rect = range.getBoundingClientRect();
    const context = range.commonAncestorContainer.parentElement?.closest("p")?.textContent ?? text;
    const halfWidth = 165;
    const clampedX = Math.max(halfWidth + 8, Math.min(rect.left + rect.width / 2, window.innerWidth - halfWidth - 8));
    const bottomSpace = window.innerHeight - rect.bottom - 16;
    const popoverHeight = 220;
    const yBelow = rect.bottom + 12;
    const yAbove = rect.top - popoverHeight - 12;
    const clampedY = bottomSpace >= popoverHeight ? yBelow : Math.max(8, yAbove);
    const placement = bottomSpace >= popoverHeight ? "below" : "above";
    const requestSequence = ++selectionRequestSequenceRef.current;
    window.clearTimeout(selectionRequestTimerRef.current);
    setSelection({ text, context, x: clampedX, y: clampedY, placement });
    setExplanation(undefined);
    setExplanationError(undefined);
    selectionRequestTimerRef.current = window.setTimeout(() => {
      void api.explainSelection(article.id, text, context)
        .then((result) => {
          if (requestSequence !== selectionRequestSequenceRef.current) return;
          setExplanation(result);
          void api.getProgress().then(setProgress);
        })
        .catch((reason) => {
          if (requestSequence !== selectionRequestSequenceRef.current) return;
          const message = String(reason);
          setExplanationError(message.includes("JSON 无效") ? "AI 返回格式异常，已自动重试仍未成功。请稍后重新划选。" : message);
        });
    }, 180);
  };

  const refreshDaily = async () => {
    if (completed || refreshingDaily) return;
    setRefreshingDaily(true);
    setDailyRefreshMessage(undefined);
    try {
      const nextArticle = await api.refreshTodayArticle();
      setArticle(nextArticle);
      setSelection(undefined);
      setExplanation(undefined);
      setExplanationError(undefined);
      setQuestions([]);
      setQuestionIndex(0);
      setAnswerState(undefined);
      setFeedback(undefined);
      window.scrollTo({ top: 0, behavior: "smooth" });
      void api.getProgress().then(setProgress);
    } catch (reason) {
      setDailyRefreshMessage(String(reason));
    } finally {
      setRefreshingDaily(false);
    }
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

  const gradeReviewCard = async (remembered: boolean) => {
    if (!reviewCard || reviewingCard) return;
    setReviewingCard(true);
    try {
      const nextCard = await api.reviewCard(reviewCard.id, remembered);
      setReviewCard(nextCard);
      setReviewRevealed(false);
    } finally {
      setReviewingCard(false);
    }
  };

  const openReviewDeck = async () => {
    try {
      const cards = await api.getReviewCards();
      setReviewCards(cards);
      setDeckIndex(0);
      setReviewRevealed(false);
      setShowReviewDeck(true);
    } catch (reason) {
      setError(String(reason));
    }
  };

  const gradeDeckCard = async (remembered: boolean) => {
    const card = reviewCards[deckIndex];
    if (!card || reviewingCard) return;
    setReviewingCard(true);
    try {
      await api.reviewCard(card.id, remembered);
      const cards = await api.getReviewCards();
      setReviewCards(cards);
      setDeckIndex((index) => Math.min(index, Math.max(cards.length - 1, 0)));
      setReviewRevealed(false);
      void api.getDueReviewCard().then(setReviewCard);
    } finally { setReviewingCard(false); }
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
      setKeyMessage({ text: "API Key、Base URL、协议和模型已保存到本机应用设置。下次划词会使用所选配置。", type: "success" });
    } catch (reason) {
      setKeyMessage({ text: String(reason), type: "error" });
    }
  };

  const detectModels = async () => {
    setDetectingModels(true);
    setKeyMessage(undefined);
    try {
      const discovered = await api.discoverModels(baseUrl, apiKey);
      setAvailableModels(discovered);
      setModel(discovered.includes(model) ? model : discovered[0]);
      setKeyMessage({ text: `已检测到 ${discovered.length} 个模型，请选择一个后保存。`, type: "info" });
    } catch (reason) {
      setAvailableModels([]);
      setKeyMessage({ text: String(reason), type: "error" });
    } finally {
      setDetectingModels(false);
    }
  };

  const saveReminder = async () => {
    const [hour, minute] = reminderTime.split(":").map(Number);
    try {
      const status = await api.installDailyReminder(hour, minute);
      setReminder(status);
      setReminderMessage({ text: `已启用，每天 ${reminderTime} 提醒。`, type: "success" });
    } catch (reason) { setReminderMessage({ text: String(reason), type: "error" }); }
  };

  const disableReminder = async () => {
    try {
      setReminder(await api.removeDailyReminder());
      setReminderMessage({ text: "每日提醒已关闭。", type: "success" });
    } catch (reason) { setReminderMessage({ text: String(reason), type: "error" }); }
  };

  const openProfile = async () => {
    try {
      const [profile, plan] = await Promise.all([api.getAbilityProfile(), api.getMultiAgentPlan()]);
      setAbilityProfile(profile);
      setMultiAgentPlan(plan);
      setTargetLevel(profile.targetLevel ?? "");
      setProfileMessage(undefined);
      setShowProfile(true);
    } catch (reason) { setError(String(reason)); }
  };

  const refreshAgentPlan = async () => {
    setRefreshingAgentPlan(true);
    setProfileMessage(undefined);
    try {
      const plan = await api.refreshMultiAgentPlan();
      setMultiAgentPlan(plan);
      setProfileMessage({ text: "学习策略已更新；下一篇选文与新题目会使用它。", type: "success" });
    } catch (reason) { setProfileMessage({ text: String(reason), type: "error" }); }
    finally { setRefreshingAgentPlan(false); }
  };

  const saveTargetLevel = async () => {
    try {
      const profile = await api.updateTargetLevel(targetLevel || undefined);
      setAbilityProfile(profile);
      setTargetLevel(profile.targetLevel ?? "");
      setProfileMessage({ text: "目标难度已保存，将从下一篇文章开始生效。", type: "success" });
    } catch (reason) { setProfileMessage({ text: String(reason), type: "error" }); }
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
    try {
      const result = await api.submitInitialAssessment(answers);
      setInitialResult(result);
      void api.getProgress().then(setProgress);
    } catch (reason) {
      setError(String(reason));
    }
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
    try {
      const result = await api.submitWeeklyAssessment(weeklyAssessment.id, answers);
      setWeeklyResult(result);
      setWeeklyAssessment({ ...weeklyAssessment, completed: true, result });
      void api.getProgress().then(setProgress);
    } catch (reason) {
      setError(String(reason));
    }
  };

  if (loading) {
    return (
      <main className="loading">
        <div className="loading-card">
          <span className="spinner" aria-hidden="true" />
          <p>正在准备今天唯一的一篇阅读…</p>
        </div>
      </main>
    );
  }

  if (error || !article) {
    return (
      <main className="loading error">
        <div className="loading-card">
          <p>启动失败：{error ?? "没有可用文章"}</p>
          <Button variant="primary" onClick={() => window.location.reload()}>重试</Button>
        </div>
      </main>
    );
  }

  return (
    <main className={`app-shell ${menuCollapsed ? "menu-collapsed" : ""}`}>
      <header className="topbar">
        <div className="brand-block">
          <p className="eyebrow">Kotoba Atelier · TODAY</p>
          <h1>读懂一点，世界就更近一点</h1>
        </div>
        <div className="metric-card hero-summary">
          <span className="metric-label">学习信号</span>
          <strong className="metric-value">
            {progress?.independentExpressionRate == null ? "积累中" : `${Math.round(progress.independentExpressionRate * 100)}%`}
          </strong>
          <div className="metric-meta">
            <span>{progress?.independentExpressionRate == null ? "等待第一次新语境重逢" : "新语境中的旧表达理解"}</span>
            <span>{selectionRateHint}</span>
            <span>完成 {progress?.completedArticles ?? 0} 篇 · 略过 {progress?.missedArticles ?? 0} 篇</span>
          </div>
        </div>
      </header>

      <nav className={`side-menu ${menuCollapsed ? "collapsed" : ""}`} aria-label="学习工具">
        <div className="side-menu-head">
          <span className="side-mark">K</span>
          <strong>Kotoba</strong>
          <button
            className="menu-toggle"
            onClick={() => setMenuCollapsed((value) => !value)}
            aria-label={menuCollapsed ? "展开学习工具" : "收起学习工具"}
            aria-expanded={!menuCollapsed}
            type="button"
          >
            {menuCollapsed ? "☰" : "‹"}
          </button>
        </div>
        <div className="menu-actions">
          <button
            className="menu-button"
            title="AI 工作台"
            aria-label="AI 工作台"
            onClick={() => {
              setBaseUrl(aiStatus?.baseUrl ?? "https://api.openai.com/v1");
              setModel(aiStatus?.model ?? "gpt-5.6-luna");
              setProtocol(aiStatus?.protocol ?? "responses");
              setShowSettings(true);
            }}
            type="button"
          >
            <MenuIcon name="ai" />
            <span>AI 工作台</span>
          </button>
          <button
            className="menu-button"
            title="独立阅读"
            aria-label="独立阅读"
            onClick={() => void openWeeklyAssessment()}
            type="button"
          >
            <MenuIcon name="weekly" />
            <span>独立阅读</span>
          </button>
          <button
            className="menu-button"
            title="复习卡片"
            aria-label="复习卡片"
            onClick={() => void openReviewDeck()}
            type="button"
          >
            <MenuIcon name="deck" />
            <span>复习卡片</span>
          </button>
          <button
            className="menu-button"
            title="学习轨迹"
            aria-label="学习轨迹"
            onClick={() => setShowProgress(true)}
            type="button"
          >
            <MenuIcon name="progress" />
            <span>学习轨迹</span>
          </button>
          <button
            className="menu-button"
            title="语言画像"
            aria-label="语言画像"
            onClick={() => void openProfile()}
            type="button"
          >
            <MenuIcon name="profile" />
            <span>语言画像</span>
          </button>
          <button
            className="menu-button"
            onClick={() => void refreshDaily()}
            disabled={completed || refreshingDaily}
            title={completed ? "完成后日报不可更换" : "换一篇未读日报"}
            aria-label={completed ? "完成后日报不可更换" : "换一篇未读日报"}
            type="button"
          >
            <MenuIcon name="refresh" />
            <span>{refreshingDaily ? "正在选文…" : "重新选文"}</span>
          </button>
        </div>
      </nav>

      {dailyRefreshMessage && <p className="daily-refresh-message">{dailyRefreshMessage}</p>}

      <section className="article-layout">
        <aside className="article-meta">
          <span className="meta-source">{article.source}</span>
          <div className="meta-row">
            <span>{article.publishedAt}</span>
            <span>{article.readingMinutes} 分钟</span>
            <span>{article.difficulty}</span>
            {article.isExploration && <span className="explore">探索题材</span>}
          </div>
          <a href={article.url} target="_blank" rel="noreferrer">
            查看原始来源 ↗
          </a>
        </aside>

        <article className="reader" ref={readerRef} onMouseUp={handleSelection} onTouchEnd={handleSelection}>
          <h2>{article.title}</h2>
          <p className="reader-tip">任意划选词、短语或句子，即可获得读音、中文语境解释，并自动生成一张明日复习卡。</p>
          {reviewCard && !inlineReviewDismissed && (
            <section className="inline-review" aria-live="polite">
              <div>
                <div>
                  <p className="eyebrow">阅读中的复现</p>
                  <small>来自《{reviewCard.articleTitle}》 · 第 {reviewCard.reviewCount + 1} 次</small>
                </div>
                <button
                  className="btn btn-ghost"
                  onClick={() => setInlineReviewDismissed(true)}
                  aria-label="关闭复习卡片"
                  type="button"
                >
                  ×
                </button>
              </div>
              <h3>{reviewCard.front}</h3>
              {!reviewRevealed ? (
                <Button variant="secondary" onClick={() => setReviewRevealed(true)}>
                  想好后显示答案
                </Button>
              ) : (
                <div className="review-answer">
                  <p><strong>读音</strong>{reviewCard.reading}</p>
                  <p><strong>意思</strong>{reviewCard.translation}</p>
                  {reviewCard.contextNote && <p><strong>语境</strong>{reviewCard.contextNote}</p>}
                  <div className="review-actions">
                    <Button variant="secondary" disabled={reviewingCard} onClick={() => void gradeReviewCard(false)}>
                      没想起来 · 明天再见
                    </Button>
                    <Button variant="primary" disabled={reviewingCard} onClick={() => void gradeReviewCard(true)}>
                      想起来了 · 延后复现
                    </Button>
                  </div>
                </div>
              )}
            </section>
          )}
          {article.paragraphs.map((paragraph, index) => (
            <section className="article-block" key={index}>
              <p>{paragraph}</p>
              {index < article.images.length && (
                <a className="article-image-link" href={article.url} target="_blank" rel="noreferrer">
                  <img src={article.images[index]} alt={`《${article.title}》配图`} referrerPolicy="no-referrer" />
                </a>
              )}
            </section>
          ))}
          {article.embeds.length > 0 && (
            <section className="article-embeds">
              {article.embeds.map((embed) =>
                embed.kind === "video" && embed.url.includes("youtube.com/embed/")
                  ? <iframe key={embed.url} src={embed.url} title={`《${article.title}》嵌入视频`} loading="lazy" allowFullScreen />
                  : <a key={embed.url} href={embed.url} target="_blank" rel="noreferrer">查看来源文章中的社媒或视频内容 ↗</a>
              )}
            </section>
          )}
          <div className="finish-zone" ref={finishRef}>
            <p className={quizError ? "error-text" : undefined}>
              {completed ? "已到达文末，理解题已出现。" : quizError ? `理解题准备失败：${quizError}` : "到达这里会自动准备理解题。"}
            </p>
            {!completed && (
              <Button variant="primary" onClick={() => void finishReading()}>
                没有自动出现？点击重试
              </Button>
            )}
          </div>
          {completed && (
            <section className="inline-quiz" aria-live="polite">
              {currentQuestion ? (
                <>
                  <p className="eyebrow">理解题 {questionIndex + 1} / {questions.length}</p>
                  <h3>{currentQuestion.prompt}</h3>
                  <div className="choices">
                    {currentQuestion.choices.map((choice, index) => {
                      const isChosen = answerState?.chosen === index;
                      const isAnswer = index === currentQuestion.answerIndex;
                      return (
                        <button
                          key={`${currentQuestion.id}-${index}`}
                          disabled={Boolean(answerState)}
                          onClick={() => void answerQuestion(index)}
                          className={answerState ? (isAnswer ? "correct" : isChosen ? "wrong" : "") : ""}
                          type="button"
                        >
                          {answerState && isAnswer && <span className="choice-label">正确答案</span>}
                          {answerState && isChosen && !isAnswer && <span className="choice-label">你的选择</span>}
                          {choice}
                        </button>
                      );
                    })}
                  </div>
                  {answerState && (
                    <div className="evidence">
                      <strong>{answerState.correct ? "答对了" : "正确答案与原文依据"}</strong>
                      <span className="evidence-label">原文依据</span>
                      <blockquote>{currentQuestion.evidence}</blockquote>
                      <span className="evidence-label">中文解析</span>
                      <p>{currentQuestion.explanation}</p>
                      {questionIndex + 1 < questions.length ? (
                        <Button variant="primary" onClick={nextQuestion}>下一题</Button>
                      ) : (
                        <div className="inline-feedback">
                          <p className="done">理解题完成。你对今天这个题材的感觉？</p>
                          {feedback ? (
                            <p className="feedback-confirmed">已记录：{feedback}</p>
                          ) : (
                            <div className="feedback-options">
                              {feedbackOptions.map((option) => (
                                <button key={option} onClick={() => void submitFeedback(option)} type="button">
                                  {option}
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  )}
                </>
              ) : (
                <p>正在准备理解题…</p>
              )}
            </section>
          )}
        </article>
      </section>

      {selection && (
        <section
          ref={selectionPopoverRef}
          className={`selection-popover placement-${selection.placement}`}
          style={{ left: selection.x, top: selection.y }}
        >
          <button
            className="popover-close"
            onClick={() => {
              selectionRequestSequenceRef.current += 1;
              window.clearTimeout(selectionRequestTimerRef.current);
              setSelection(undefined);
              setExplanation(undefined);
              setExplanationError(undefined);
            }}
            aria-label="关闭"
            type="button"
          >
            ×
          </button>
          <p className="selected">「{selection.text}」</p>
          {explanation ? (
            <>
              <p className="reading"><strong>读音</strong> {explanation.reading}</p>
              <p className="translation"><strong>译文</strong> {explanation.translation}</p>
              {explanation.contextNote && <p><strong>语境</strong> {explanation.contextNote}</p>}
              {explanation.example && (
                <div className="example">
                  <strong>例句</strong>
                  <p>{explanation.example}</p>
                  {explanation.exampleTranslation && <p>{explanation.exampleTranslation}</p>}
                </div>
              )}
              {explanation.grammarNote && <p className="grammar"><strong>语法/搭配</strong> {explanation.grammarNote}</p>}
            </>
          ) : explanationError ? (
            <p className="inline-error">
              生成失败：{explanationError}<br />请关闭后重新划选一次。
            </p>
          ) : (
            <p className="thinking">正在结合上下文生成解释…</p>
          )}
        </section>
      )}

      <Modal open={showReviewDeck} onClose={() => setShowReviewDeck(false)} size="full">
        <div className="review-deck-page">
          <header>
            <p className="eyebrow">KOTOBA ATELIER · CARDS</p>
            <h2>复习卡片</h2>
            <p>AI 已从你的划词中筛出值得反复遇见的表达。</p>
          </header>
          {deckCard ? (
            <div className="review-deck-layout">
              <div className="review-stack" aria-label="错位叠放的复习卡">
                <i />
                <i />
                <article className="deck-card">
                  <small>第 {deckIndex + 1} / {reviewCards.length} 张 · 来自《{deckCard.articleTitle}》</small>
                  <h3>{deckCard.front}</h3>
                  {!reviewRevealed ? (
                    <Button variant="secondary" className="deck-reveal" onClick={() => setReviewRevealed(true)}>
                      先在脑中回答，再翻开
                    </Button>
                  ) : (
                    <div className="deck-back">
                      <p><strong>读音</strong>{deckCard.reading}</p>
                      <p><strong>意思</strong>{deckCard.translation}</p>
                      {deckCard.contextNote && <p><strong>语境</strong>{deckCard.contextNote}</p>}
                      <div className="deck-grade">
                        <Button variant="secondary" disabled={reviewingCard} onClick={() => void gradeDeckCard(false)}>
                          没想起
                        </Button>
                        <Button variant="primary" disabled={reviewingCard} onClick={() => void gradeDeckCard(true)}>
                          想起来了
                        </Button>
                      </div>
                    </div>
                  )}
                </article>
              </div>
              <aside className="deck-index">
                <strong>{reviewCards.length} 张已整理</strong>
                <span>只保留 AI 判断为可复用、非重复的表达。</span>
                <div>
                  {reviewCards.slice(0, 12).map((card, index) => (
                    <button
                      key={card.id}
                      className={index === deckIndex ? "active" : ""}
                      onClick={() => { setDeckIndex(index); setReviewRevealed(false); }}
                      type="button"
                    >
                      {card.front}
                    </button>
                  ))}
                </div>
                {reviewCards.length > 12 && <small>共 {reviewCards.length} 张，此处列出前 12 张</small>}
              </aside>
            </div>
          ) : (
            <div className="deck-empty">
              <h3>还没有可复习的卡片</h3>
              <p>继续阅读并划词。AI 会去重、拆解后，只把值得反复遇见的表达放进这里。</p>
            </div>
          )}
        </div>
      </Modal>

      <Modal open={showSettings} onClose={() => setShowSettings(false)} titleId="ai-settings-title" size="lg">
        <div className="settings-hero">
          <p className="eyebrow">语言引擎</p>
          <h3 id="ai-settings-title">AI 工作台</h3>
          <p className={`settings-status ${aiStatus?.configured ? "configured" : ""}`}>
            <i />
            {aiStatus?.configured
              ? `已连接 · ${aiStatus.model} · ${aiStatus.protocol === "chat_completions" ? "Chat Completions" : "Responses"}`
              : "尚未连接 · 将使用本地提示"}
          </p>
        </div>
        <section className="settings-section">
          <div className="section-heading">
            <strong>连接配置</strong>
            <small>选择与你的 API 网关匹配的调用方式。</small>
          </div>
          <div className="ai-config-grid">
            <div className="wide-field">
              <label className="field-label" htmlFor="base-url">
                服务地址 <em>Base URL</em>
              </label>
              <input
                id="base-url"
                type="url"
                value={baseUrl}
                onChange={(event) => { setBaseUrl(event.target.value); setAvailableModels([]); }}
                placeholder="https://api.openai.com/v1"
                autoComplete="url"
              />
            </div>
            <div>
              <label className="field-label" htmlFor="protocol">调用方式</label>
              <select
                id="protocol"
                value={protocol}
                onChange={(event) => setProtocol(event.target.value as "responses" | "chat_completions")}
              >
                <option value="responses">Responses</option>
                <option value="chat_completions">Chat Completions</option>
              </select>
            </div>
            <div>
              <label className="field-label" htmlFor="model">使用模型</label>
              <select
                id="model"
                value={model}
                onChange={(event) => setModel(event.target.value)}
                disabled={availableModels.length === 0}
              >
                {availableModels.length === 0 ? (
                  <option>先刷新模型列表</option>
                ) : (
                  availableModels.map((name) => <option key={name} value={name}>{name}</option>)
                )}
              </select>
            </div>
            <div className="wide-field">
              <label className="field-label" htmlFor="api-key">
                访问密钥 <em>API Key</em>
              </label>
              <input
                id="api-key"
                type="password"
                value={apiKey}
                onChange={(event) => { setApiKey(event.target.value); setAvailableModels([]); }}
                placeholder={aiStatus?.configured ? "已保存到本机；仅在替换时输入" : "粘贴 API Key"}
                autoComplete="off"
              />
            </div>
          </div>
          <div className="settings-actions">
            <Button
              variant="secondary"
              onClick={() => void detectModels()}
              disabled={!baseUrl.trim() || (!apiKey.trim() && !aiStatus?.configured) || detectingModels}
            >
              {detectingModels ? "正在刷新…" : "刷新模型列表"}
            </Button>
            <Button
              variant="primary"
              onClick={() => void saveKey()}
              disabled={!baseUrl.trim() || availableModels.length === 0 || (!apiKey.trim() && !aiStatus?.configured)}
            >
              保存连接
            </Button>
          </div>
          {(!baseUrl.trim() || availableModels.length === 0 || (!apiKey.trim() && !aiStatus?.configured)) && (
            <p className="helper-text">
              保存连接需要：已填写服务地址、完成模型检测并选择模型、已填写 API Key（或本机已保存）。
            </p>
          )}
          <p className="settings-note">
            仅允许 HTTPS 网关。划词时会发送所划文本及附近上下文；生成理解题会发送当前文章全文和历史划词表达；生成学习策略会发送本地成绩、划词汇总与题材反馈。API Key 与其他配置只保存在本机应用设置（不访问 macOS Keychain）。tokendance 网关请使用 Chat Completions。
          </p>
          {keyMessage && <p className={`message message-${keyMessage.type}`}>{keyMessage.text}</p>}
        </section>
        <section className="settings-section reminder-section">
          <div className="section-heading">
            <strong>每日阅读提醒</strong>
            <small>{reminder.enabled ? `已安排在 ${String(reminder.hour).padStart(2, "0")}:${String(reminder.minute).padStart(2, "0")}` : "为明天的唯一阅读留一个固定入口。"}</small>
          </div>
          <label className="field-label" htmlFor="reminder-time">提醒时间</label>
          <input
            id="reminder-time"
            type="time"
            value={reminderTime}
            onChange={(event) => setReminderTime(event.target.value)}
          />
          <div className="reminder-actions">
            {reminder.enabled && (
              <Button variant="secondary" onClick={() => void disableReminder()}>
                暂停提醒
              </Button>
            )}
            <Button variant="primary" onClick={() => void saveReminder()}>
              {reminder.enabled ? "更新提醒" : "启用提醒"}
            </Button>
          </div>
          {reminderMessage && <p className={`message message-${reminderMessage.type}`}>{reminderMessage.text}</p>}
        </section>
      </Modal>

      <Modal open={showProgress} onClose={() => setShowProgress(false)} titleId="progress-title" size="xl">
        <p className="eyebrow">最近 14 天</p>
        <h3 id="progress-title">阅读辅助依赖</h3>
        <div className={`experiment-status ${progress?.experiment.readyForVerdict ? "ready" : "collecting"}`}>
          <div>
            <span>两周试验</span>
            <strong>{progress?.experiment.verdict ?? "证据积累中"}</strong>
          </div>
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
        {progress?.selectionTrend.length ? (
          <div className="trend-chart" aria-label="归一化划词频率曲线" tabIndex={0}>
            {progress.selectionTrend.map((point, index) => {
              const max = Math.max(...progress.selectionTrend.map((item) => item.normalizedRate), 1);
              return (
                <div className="trend-column" key={`${point.day}-${index}`} title={`${point.day}：${point.normalizedRate.toFixed(1)} 次/千字`}>
                  <span>{point.normalizedRate.toFixed(1)}</span>
                  <i style={{ height: `${Math.max(5, point.normalizedRate / max * 120)}px` }} />
                  <small>{point.day.slice(5)}</small>
                </div>
              );
            })}
          </div>
        ) : (
          <p className="empty-trend">完成阅读并划词后，这里会形成趋势。</p>
        )}
        <h3 className="trend-subtitle">独立阅读理解</h3>
        {progress?.assessmentTrend.length ? (
          <div className="assessment-history">
            {progress.assessmentTrend.map((point) => (
              <div key={point.week}>
                <span>{point.week}</span>
                <strong>{Math.round(point.scoreRate * 100)}%</strong>
              </div>
            ))}
          </div>
        ) : (
          <p className="empty-trend">完成本周独立评估后开始记录。</p>
        )}
      </Modal>

      {showProfile && abilityProfile && (
        <Modal open={showProfile} onClose={() => setShowProfile(false)} titleId="profile-title" size="lg">
          <p className="eyebrow">可查看 · 可校正</p>
          <h3 id="profile-title">基础能力画像</h3>
          <p>这是学习证据的汇总，不是永久 JLPT 标签。人工设置只控制未来选文目标难度。</p>
          <div className="profile-grid">
            <div><span>系统建议</span><strong>{abilityProfile.suggestedLevel}</strong></div>
            <div><span>完成文章</span><strong>{abilityProfile.completedArticles}</strong></div>
            <div><span>初始定位</span><strong>{abilityProfile.initialScore == null ? "待测" : `${Math.round(abilityProfile.initialScore * 100)}%`}</strong></div>
            <div><span>日常理解题</span><strong>{abilityProfile.dailyAccuracy == null ? "证据不足" : `${Math.round(abilityProfile.dailyAccuracy * 100)}%`}</strong></div>
            <div><span>独立周测</span><strong>{abilityProfile.weeklyAccuracy == null ? "证据不足" : `${Math.round(abilityProfile.weeklyAccuracy * 100)}%`}</strong></div>
            <div><span>中文展开率</span><strong>{abilityProfile.chineseRevealRate == null ? "证据不足" : `${Math.round(abilityProfile.chineseRevealRate * 100)}%`}</strong></div>
          </div>
          <section className="agent-plan-card">
            <div className="agent-plan-heading">
              <div>
                <p className="eyebrow">学习策略编排</p>
                <h4>下一阶段学习策略</h4>
              </div>
              <Button
                variant="secondary"
                onClick={() => void refreshAgentPlan()}
                disabled={refreshingAgentPlan}
              >
                {refreshingAgentPlan ? "正在分析…" : multiAgentPlan ? "更新策略" : "生成策略"}
              </Button>
            </div>
            {multiAgentPlan ? (
              <>
                <p><strong>学习分析师</strong>{multiAgentPlan.rationale}</p>
                <p><strong>内容策展人</strong>{multiAgentPlan.articleBrief}</p>
                <p><strong>出题教练</strong>{multiAgentPlan.questionBrief}</p>
                <div className="agent-plan-tags">
                  <span>目标 {multiAgentPlan.targetDifficulty}</span>
                  {multiAgentPlan.focusTerms.map((term) => <span key={term}>{term}</span>)}
                </div>
                {multiAgentPlan.avoidTerms.length > 0 && (
                  <p className="agent-plan-caution"><strong>暂缓</strong>{multiAgentPlan.avoidTerms.join("、")}</p>
                )}
              </>
            ) : (
              <p className="agent-plan-empty">根据你的划词、题目、完成与题材反馈，生成下一篇文章和下一组题目的协作策略。</p>
            )}
          </section>
          <label className="field-label" htmlFor="target-level">未来文章目标难度</label>
          <select id="target-level" value={targetLevel} onChange={(event) => setTargetLevel(event.target.value)}>
            <option value="">自动判断</option>
            {["N5", "N4", "N3", "N2", "N1"].map((level) => <option key={level} value={level}>{level}</option>)}
          </select>
          <p className="helper-text">选择具体等级后，AI 会优先挑选该难度的文章；留空则使用系统建议。</p>
          <Button variant="primary" onClick={() => void saveTargetLevel()}>保存校正</Button>
          {profileMessage && <p className={`message message-${profileMessage.type}`}>{profileMessage.text}</p>}
        </Modal>
      )}

      {showWeeklyAssessment && weeklyAssessment && (
        <Modal open={showWeeklyAssessment} onClose={() => setShowWeeklyAssessment(false)} titleId="weekly-title" size="xl">
          <p className="eyebrow">每周独立评估 · {weeklyAssessment.week}</p>
          <h3 id="weekly-title">不使用翻译，读完这篇新文章</h3>
          <p className="weekly-rule">评估期间不提供划词解释或中文翻译。结果与日常辅助阅读分开记录。</p>
          {weeklyResult ? (
            <div className="assessment-result">
              <strong>{weeklyResult.score} / {weeklyResult.total}</strong>
              <p>{weeklyResult.levelHint}</p>
            </div>
          ) : weeklyQuestionIndex < 0 ? (
            <>
              <p className="weekly-title">{weeklyAssessment.article.title}</p>
              <div className="weekly-reader">{weeklyAssessment.article.paragraphs.map((paragraph, index) => <p key={index}>{paragraph}</p>)}</div>
              <Button variant="primary" onClick={startWeeklyQuestions}>我已读完，开始理解题</Button>
            </>
          ) : (
            <>
              <p className="assessment-prompt">{weeklyAssessment.questions[weeklyQuestionIndex]?.prompt}</p>
              <div className="assessment-options">
                {weeklyAssessment.questions[weeklyQuestionIndex]?.choices.map((choice, index) => (
                  <button key={`${weeklyQuestionIndex}-${choice}`} onClick={() => void answerWeeklyQuestion(index)} type="button">
                    {choice}
                  </button>
                ))}
              </div>
              <small>{weeklyQuestionIndex + 1} / {weeklyAssessment.questions.length}</small>
            </>
          )}
        </Modal>
      )}

      {showCalibration && candidates[candidateIndex] && (
        <Modal open={showCalibration} onClose={() => setShowCalibration(false)} titleId="calibration-title" size="md">
          <p className="eyebrow">兴趣冷启动 {candidateIndex + 1} / {Math.min(candidates.length, 8)}</p>
          <h3 id="calibration-title">这个标题会想读吗？</h3>
          <p className="candidate-title">{candidates[candidateIndex].title}</p>
          <small>AI 会用这些快速反馈挑选今后的文章；真实阅读、划词和测验的权重会更高。</small>
          <div className="calibration-actions">
            <Button variant="secondary" onClick={() => void voteForTitle("不想看")}>不想看</Button>
            <Button variant="secondary" onClick={() => void voteForTitle("无感")}>无感</Button>
            <Button variant="primary" onClick={() => void voteForTitle("想看")}>想看</Button>
          </div>
        </Modal>
      )}

      {showInitialAssessment && initialQuestions[initialIndex] && (
        <Modal open={showInitialAssessment} onClose={() => setShowInitialAssessment(false)} titleId={initialResult ? "initial-result-title" : "initial-title"} size="lg">
          {!initialResult ? (
            <>
              <p className="eyebrow">初始定位 {initialIndex + 1} / {initialQuestions.length}</p>
              <h3 id="initial-title">先建立你的阅读起点</h3>
              <p className="assessment-prompt">{initialQuestions[initialIndex].prompt}</p>
              <div className="assessment-options">
                {initialQuestions[initialIndex].choices.map((choice, index) => (
                  <button key={`${initialIndex}-${choice}`} onClick={() => void answerInitialQuestion(index)} type="button">
                    {choice}
                  </button>
                ))}
              </div>
              <small>这不是永久等级标签。今后的划词、真实阅读和每周独立评估会持续修正它。</small>
            </>
          ) : (
            <>
              <p className="eyebrow">定位完成</p>
              <h3 id="initial-result-title">{initialResult.score} / {initialResult.total}</h3>
              <p>{initialResult.levelHint}</p>
              <Button variant="primary" onClick={() => setShowInitialAssessment(false)}>开始今天的阅读</Button>
            </>
          )}
        </Modal>
      )}
    </main>
  );
}
