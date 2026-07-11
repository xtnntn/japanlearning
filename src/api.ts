import { invoke } from "@tauri-apps/api/core";

export type Article = {
  id: string;
  title: string;
  source: string;
  url: string;
  publishedAt: string;
  paragraphs: string[];
  images: string[];
  embeds: { kind: string; url: string }[];
  readingMinutes: number;
  difficulty: string;
  isExploration: boolean;
};

export type Explanation = {
  reading: string;
  translation: string;
  contextNote: string;
  example: string;
  exampleTranslation: string;
  grammarNote: string;
};

export type Question = {
  id: string;
  prompt: string;
  choices: string[];
  answerIndex: number;
  evidence: string;
  explanation: string;
  testedExpressions: string[];
};

export type Progress = {
  selectedCount: number;
  chineseReveals: number;
  completedArticles: number;
  missedArticles: number;
  titleVotes: number;
  baselineCompleted: boolean;
  topicFeedback: { label: string; count: number }[];
  selectionTrend: { day: string; normalizedRate: number; selections: number; characterCount: number }[];
  assessmentTrend: { week: string; scoreRate: number }[];
  independentExpressionRate?: number;
  independentExpressionAttempts: number;
  experiment: { observedDays: number; completedDays: number; selectionRateChange?: number; weeklyScoreNonDeclining?: boolean; expressionRateChange?: number; readyForVerdict: boolean; verdict: string };
};
export type ReminderStatus = { enabled: boolean; hour: number; minute: number };
export type AbilityProfile = { suggestedLevel: string; targetLevel?: string; initialScore?: number; dailyAccuracy?: number; weeklyAccuracy?: number; selectionCount: number; chineseRevealRate?: number; completedArticles: number };

export type TitleCandidate = { id: string; title: string; url: string; source: string };
export type AssessmentQuestion = { id: string; prompt: string; choices: string[]; answerIndex: number };
export type AssessmentResult = { score: number; total: number; levelHint: string };
export type WeeklyAssessment = { id: string; week: string; article: Article; questions: Question[]; completed: boolean; result?: AssessmentResult };

export type AiStatus = { configured: boolean; model: string; baseUrl: string; protocol: "responses" | "chat_completions" };

export const api = {
  getTodayArticle: () => invoke<Article>("get_today_article"),
  refreshTodayArticle: () => invoke<Article>("refresh_today_article"),
  explainSelection: (articleId: string, selection: string, context: string) =>
    invoke<Explanation>("explain_selection", { articleId, selection, context }),
  getQuestions: (article: Article) => invoke<Question[]>("get_questions", { article }),
  recordAnswer: (articleId: string, questionId: string, chosenIndex: number, answerIndex: number, testedExpressions: string[]) =>
    invoke<boolean>("record_answer", { articleId, questionId, chosenIndex, answerIndex, testedExpressions }),
  completeArticle: (articleId: string) => invoke<void>("complete_article", { articleId }),
  saveTopicFeedback: (articleId: string, label: string) =>
    invoke<void>("save_topic_feedback", { articleId, label }),
  getProgress: () => invoke<Progress>("get_progress"),
  getAiStatus: () => invoke<AiStatus>("get_ai_status"),
  discoverModels: (baseUrl: string, apiKey: string) => invoke<string[]>("discover_models", { baseUrl, apiKey }),
  saveOpenAiApiKey: (apiKey: string, baseUrl: string, model: string, protocol: "responses" | "chat_completions") => invoke<void>("save_openai_api_key", { apiKey, baseUrl, model, protocol }),
  getTitleCandidates: () => invoke<TitleCandidate[]>("get_title_candidates"),
  saveTitleVote: (candidate: TitleCandidate, vote: string) => invoke<void>("save_title_vote", { candidate, vote }),
  getInitialAssessment: () => invoke<AssessmentQuestion[]>("get_initial_assessment"),
  submitInitialAssessment: (answers: number[]) => invoke<AssessmentResult>("submit_initial_assessment", { answers }),
  getWeeklyAssessment: () => invoke<WeeklyAssessment>("get_weekly_assessment"),
  submitWeeklyAssessment: (assessmentId: string, answers: number[]) => invoke<AssessmentResult>("submit_weekly_assessment", { assessmentId, answers })
  ,getReminderStatus: () => invoke<ReminderStatus>("get_reminder_status")
  ,installDailyReminder: (hour: number, minute: number) => invoke<ReminderStatus>("install_daily_reminder", { hour, minute })
  ,removeDailyReminder: () => invoke<ReminderStatus>("remove_daily_reminder")
  ,getAbilityProfile: () => invoke<AbilityProfile>("get_ability_profile")
  ,updateTargetLevel: (targetLevel?: string) => invoke<AbilityProfile>("update_target_level", { targetLevel: targetLevel || null })
};
