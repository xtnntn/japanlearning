import { invoke } from "@tauri-apps/api/core";

export type Article = {
  id: string;
  title: string;
  source: string;
  url: string;
  publishedAt: string;
  paragraphs: string[];
  images: string[];
  readingMinutes: number;
  difficulty: string;
  isExploration: boolean;
};

export type Explanation = {
  japaneseHint: string;
  chineseTranslation: string;
  furigana: string;
  note: string;
};

export type Question = {
  id: string;
  prompt: string;
  choices: string[];
  answerIndex: number;
  evidence: string;
  explanation: string;
};

export type Progress = {
  selectedCount: number;
  chineseReveals: number;
  completedArticles: number;
  titleVotes: number;
  baselineCompleted: boolean;
  topicFeedback: { label: string; count: number }[];
};

export type TitleCandidate = { id: string; title: string; url: string; source: string };
export type AssessmentQuestion = { id: string; prompt: string; choices: string[]; answerIndex: number };
export type AssessmentResult = { score: number; total: number; levelHint: string };

export type AiStatus = { configured: boolean; model: string };

export const api = {
  getTodayArticle: () => invoke<Article>("get_today_article"),
  explainSelection: (articleId: string, selection: string, context: string, chineseRevealed = false) =>
    invoke<Explanation>("explain_selection", { articleId, selection, context, chineseRevealed }),
  getQuestions: (article: Article) => invoke<Question[]>("get_questions", { article }),
  recordAnswer: (articleId: string, questionId: string, chosenIndex: number, answerIndex: number) =>
    invoke<boolean>("record_answer", { articleId, questionId, chosenIndex, answerIndex }),
  completeArticle: (articleId: string) => invoke<void>("complete_article", { articleId }),
  saveTopicFeedback: (articleId: string, label: string) =>
    invoke<void>("save_topic_feedback", { articleId, label }),
  getProgress: () => invoke<Progress>("get_progress"),
  getAiStatus: () => invoke<AiStatus>("get_ai_status"),
  saveOpenAiApiKey: (apiKey: string) => invoke<void>("save_openai_api_key", { apiKey }),
  getTitleCandidates: () => invoke<TitleCandidate[]>("get_title_candidates"),
  saveTitleVote: (candidate: TitleCandidate, vote: string) => invoke<void>("save_title_vote", { candidate, vote }),
  getInitialAssessment: () => invoke<AssessmentQuestion[]>("get_initial_assessment"),
  submitInitialAssessment: (answers: number[]) => invoke<AssessmentResult>("submit_initial_assessment", { answers })
};
