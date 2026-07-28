// Mirrors the Rust DTOs in src-tauri/src/dto.rs. Kept hand-written rather than generated so the
// frontend can be read on its own; the Rust tests assert the shapes these names depend on.

export type LessonLevel =
  | 'orientation'
  | 'beginner'
  | 'foundation'
  | 'intermediate'
  | 'advanced'
  | 'administrator'
  | 'troubleshooting'
  | 'capstone';

export type LessonType =
  | 'concept'
  | 'demonstration'
  | 'guided-practice'
  | 'independent-practice'
  | 'debugging'
  | 'scenario'
  | 'review'
  | 'assessment'
  | 'capstone';

export type TaskKind = 'guided' | 'independent' | 'mistake' | 'applied' | 'assessment';

export type MasteryStatus =
  | 'mastered'
  | 'strong'
  | 'passed'
  | 'needs-review'
  | 'review-required';

export type LessonStatus = 'not-started' | 'in-progress' | 'passed' | 'needs-review';

export type ProgressionMode = 'guided-path' | 'open-library' | 'assessment';

export type NetworkMode = 'disabled' | 'internal-lab' | 'restricted-internet';

export type VmState =
  | 'stopped'
  | 'starting'
  | 'booting-guest'
  | 'ready'
  | 'paused'
  | 'stopping'
  | 'unbootable'
  | 'failed';

export interface VmStatus {
  state: VmState;
  accel: 'whpx' | 'tcg';
  machine: 'microvm' | 'q35';
  memoryMb: number;
  pid?: number;
  bootMillis?: number;
  guestKernel?: string;
  imageVersion?: string;
  detail?: string;
}

export interface LessonEnvironment {
  profile: string;
  resetPolicy: 'per-attempt' | 'per-lesson' | 'manual' | 'never';
  networkMode: NetworkMode;
  sudoAllowed: boolean;
  dangerousAllowed?: boolean;
  memoryMb?: number;
  namespaces?: string[];
  setupScript?: string;
  resetScript?: string;
  fixtures?: string[];
}

export interface Demonstration {
  command: string;
  explanation?: string;
  output?: string;
}

export interface CommandOption {
  option: string;
  meaning: string;
}

export interface LessonSummary {
  remember?: string[];
  commonOptions?: CommandOption[];
  dangerous?: string[];
  related?: string[];
}

export interface TaskView {
  id: string;
  kind: TaskKind;
  instruction: string;
  context?: string;
  brokenCommand?: string;
  hintCount: number;
  optional: boolean;
  requirements: string[];
}

export interface ReviewQuestionView {
  index: number;
  type: 'multiple-choice' | 'multiple-select' | 'short-answer' | 'command-recall';
  question: string;
  answers: string[];
}

export interface LessonView {
  id: string;
  title: string;
  level: LessonLevel;
  module: string;
  type: LessonType;
  estimatedDifficulty: number;
  estimatedMinutes?: number;
  prerequisites: string[];
  concepts: string[];
  commands: string[];
  environment: LessonEnvironment;
  purpose: string;
  mentalModel: string;
  syntax: string[];
  demonstration: Demonstration[];
  explanationMarkdown?: string;
  summary?: LessonSummary;
  tasks: TaskView[];
  reviewQuestions: ReviewQuestionView[];
  hintsAvailable: boolean;
}

export interface LessonSummaryView {
  id: string;
  title: string;
  type: LessonType;
  estimatedDifficulty: number;
  estimatedMinutes?: number;
  commands: string[];
  status?: LessonStatus;
  mastery?: MasteryStatus;
  unlocked: boolean;
  missingPrerequisites: string[];
}

export interface ModuleView {
  id: string;
  number: number;
  title: string;
  level: LessonLevel;
  summary: string;
  outcomes: string[];
  pack: string;
  lessons: LessonSummaryView[];
  completedLessons: number;
}

export type Severity = 'blocking' | 'warning' | 'info';

export type RecoveryAction =
  | 'none'
  | 'verify-runtime-files'
  | 'reinstall-runtime'
  | 'repair-user-overlay'
  | 'reset-practice-environment'
  | 'restore-last-snapshot'
  | 'free-disk-space'
  | 'export-diagnostic-report';

export interface Finding {
  severity: Severity;
  title: string;
  detail: string;
  action: RecoveryAction;
}

export interface HealthReport {
  findings: Finding[];
  uncleanShutdown: boolean;
  freeDiskBytes?: number;
}

export interface Bootstrap {
  appVersion: string;
  runtimeVersion: string;
  profileId: number;
  mode: ProgressionMode;
  modules: ModuleView[];
  coreLessonCount: number;
  completedCoreLessons: number;
  masteryPercent: number;
  nextLessonId?: string;
  reviewLessonIds: string[];
  recentCommands: string[];
  vm: VmStatus;
  acceleration: string;
  health: HealthReport;
  catalogWarnings: string[];
}

export type FailureCategory =
  | 'command_not_found'
  | 'invalid_option'
  | 'wrong_working_directory'
  | 'wrong_path'
  | 'permission_denied'
  | 'file_already_exists'
  | 'wrong_file_type'
  | 'wrong_file_contents'
  | 'wrong_ownership'
  | 'wrong_permissions'
  | 'process_not_running'
  | 'service_not_active'
  | 'wrong_port'
  | 'incorrect_redirect'
  | 'pipeline_output_incorrect'
  | 'script_syntax_failure'
  | 'script_logic_failure'
  | 'network_unreachable'
  | 'dns_failure'
  | 'task_partially_completed';

export interface CheckOutcome {
  kind: string;
  passed: boolean;
  message: string;
  failureCategory?: FailureCategory;
  observed?: string;
  expected?: string;
  weight: number;
  errored: boolean;
}

export interface TaskValidation {
  lessonId: string;
  taskId: string;
  passed: boolean;
  outcomes: CheckOutcome[];
  completionPercent: number;
  primaryFailure?: CheckOutcome;
  errored: boolean;
}

export interface CheckResult {
  validation: TaskValidation;
  headline: string;
  category?: FailureCategory;
  guidance?: string;
  details: string[];
  partial: boolean;
  authoringError: boolean;
  lessonComplete: boolean;
  mastery?: MasteryStatus;
  diagnosis?: string;
}

export type HintResponse =
  | { kind: 'hint'; index: number; text: string; remaining: number; solutionNext: boolean }
  | { kind: 'solutionAvailable' }
  | { kind: 'unavailable'; reason: string };

export interface SolutionResponse {
  solution?: string;
  reason?: string;
}

export interface ReviewGrade {
  correct: boolean;
  explanation?: string;
}

export interface DirEntryInfo {
  name: string;
  fileType: string;
  size: number;
  mode: string;
  owner: string;
  group: string;
  linkTarget?: string;
}

export interface GuestDiagnostics {
  hostname: string;
  kernel: string;
  uptimeSeconds: number;
  loadAverage: [number, number, number];
  memoryTotalKb: number;
  memoryAvailableKb: number;
  rootDiskUsedPercent: number;
  rootInodesUsedPercent: number;
  failedUnits: string[];
  listeningPorts: number[];
  currentDirectory?: string;
}

export interface ProgressReport {
  lessonsAttempted: number;
  lessonsPassed: number;
  lessonsMastered: number;
  needsReview: number;
  hintsUsed: number;
  commandsMastered: string[];
  commonFailures: [string, number][];
  achievements: [string, number][];
  practiceSeconds: number;
}

export interface CommandEntry {
  command: string;
  lessons: [string, string][];
}

export type SnapshotRow = [number, string, string, number];
