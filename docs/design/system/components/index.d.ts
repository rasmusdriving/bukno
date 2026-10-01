import type * as React from 'react';

export type Provider = 'codex' | 'claude';
export type GlyphState = 'working' | 'breathe' | 'active' | 'waiting' | 'done' | 'pending' | 'queued' | 'failed' | 'unknown' | 'reconnecting' | 'stopped';

/** Stroke icon on a 16px grid, 1.5px stroke, drawn in currentColor. */
export interface IconProps { name: 'plus' | 'chevron-down' | 'chevron-up' | 'chevron-right' | 'chevron-left' | 'arrow-up' | 'arrow-down' | 'arrow-left' | 'stop' | 'bolt' | 'bolt-outline' | 'check' | 'search' | 'sidebar' | 'panel-right' | 'compose' | 'folder' | 'x' | 'external' | 'branch' | 'file' | 'alert' | 'info' | 'lock' | 'shield' | 'clock' | 'refresh' | 'terminal' | 'edit' | 'eye' | 'settings' | 'more' | 'copy' | 'image' | 'user' | 'globe' | 'tasks'; size?: number; color?: string; label?: string; className?: string; style?: React.CSSProperties }
export declare function Icon(props: IconProps): React.ReactElement;

/** Placeholder provider mark. Swap for the official mark at implementation. */
export interface ProviderMarkProps { provider: Provider; size?: number; tone?: 'color' | 'muted' | 'strong' }
export declare function ProviderMark(props: ProviderMarkProps): React.ReactElement;

/** Run and task state as a 16px glyph. `working` spins, `breathe` pulses, `still` stops both. */
export interface StatusGlyphProps { state: GlyphState; provider?: Provider; size?: number; still?: boolean; label?: string }
export declare function StatusGlyph(props: StatusGlyphProps): React.ReactElement;

/** Glyph, state word and optional elapsed time for a chat header. */
export interface StateChipProps { state: GlyphState; provider?: Provider; label?: string; time?: string; still?: boolean }
export declare function StateChip(props: StateChipProps): React.ReactElement;

export interface ButtonProps { variant?: 'primary' | 'secondary' | 'ghost' | 'danger'; size?: 'md' | 'sm'; icon?: IconProps['name']; iconRight?: IconProps['name']; kbd?: string | string[]; disabled?: boolean; ariaLabel?: string; children?: React.ReactNode; style?: React.CSSProperties; onClick?: () => void }
export declare function Button(props: ButtonProps): React.ReactElement;

export interface IconButtonProps { icon: IconProps['name']; label: string; variant?: 'ghost' | 'raised' | 'primary'; size?: 'md' | 'sm'; active?: boolean; pressed?: boolean; disabled?: boolean; onClick?: () => void }
export declare function IconButton(props: IconButtonProps): React.ReactElement;

export interface KbdProps { keys?: string | string[]; children?: React.ReactNode }
export declare function Kbd(props: KbdProps): React.ReactElement;

export interface SwitchProps { checked?: boolean; disabled?: boolean; label: string; provider?: Provider }
export declare function Switch(props: SwitchProps): React.ReactElement;

export interface SectionLabelProps { children?: React.ReactNode; label?: string; action?: IconProps['name']; actionLabel?: string }
export declare function SectionLabel(props: SectionLabelProps): React.ReactElement;

/** Sidebar chat row: title plus owning-provider mark, or a live state glyph in the same slot. */
export interface ChatRowProps { title: string; provider: Provider; state?: 'idle' | 'working' | 'waiting' | 'unread' | 'failed' | 'unavailable'; selected?: boolean; child?: boolean; focused?: boolean; icon?: IconProps['name']; kbd?: string | string[]; still?: boolean }
export declare function ChatRow(props: ChatRowProps): React.ReactElement;

export interface ProjectRowProps { name: string; open?: boolean; count?: number }
export declare function ProjectRow(props: ProjectRowProps): React.ReactElement;

/** Cached usage for one provider window. Missing data is `unavailable`, never zero. */
export interface UsageMeterProps { provider: Provider; label?: string; window?: string; left?: number; state?: 'fresh' | 'stale' | 'unavailable'; updated?: string; reset?: string }
export declare function UsageMeter(props: UsageMeterProps): React.ReactElement;

export interface ProfileRowProps { name?: string; initial?: string; open?: boolean }
export declare function ProfileRow(props: ProfileRowProps): React.ReactElement;

export interface UserMessageProps { children?: React.ReactNode; text?: string; files?: Array<{ name: string; icon?: IconProps['name'] }>; note?: string; noteIcon?: IconProps['name'] }
export declare function UserMessage(props: UserMessageProps): React.ReactElement;

export interface AgentTurnProps { provider: Provider; name?: string; meta?: string; children?: React.ReactNode }
export declare function AgentTurn(props: AgentTurnProps): React.ReactElement;

/** One item of the provider's own plan or todo list. */
export interface PlanStepProps { state: 'done' | 'active' | 'pending' | 'failed'; title?: string; detail?: string; provider?: Provider; children?: React.ReactNode }
export declare function PlanStep(props: PlanStepProps): React.ReactElement;

export interface ActivityLineProps { icon?: IconProps['name']; meta?: string; toggle?: boolean; children?: React.ReactNode; text?: string }
export declare function ActivityLine(props: ActivityLineProps): React.ReactElement;

export interface CodeBlockProps { lang?: string; code?: string; children?: React.ReactNode }
export declare function CodeBlock(props: CodeBlockProps): React.ReactElement;

export interface NoticeProps { tone?: 'info' | 'error' | 'unknown' | 'reconnecting' | 'waiting'; title: string; text?: string; children?: React.ReactNode; actions?: Array<{ label: string; variant?: ButtonProps['variant']; icon?: IconProps['name']; kbd?: string }>; still?: boolean }
export declare function Notice(props: NoticeProps): React.ReactElement;

/** The assignment a delegated task received from its parent, shown at the top of the child chat. */
export interface TaskBriefProps { from: Provider; label?: string; tag?: string; text?: string; children?: React.ReactNode }
export declare function TaskBrief(props: TaskBriefProps): React.ReactElement;

/** A Bukno-level event in a transcript: a result arrived, a task was opened, a revision was sent. */
export interface EventLineProps { state?: GlyphState; provider?: Provider; tag?: string; action?: string; text?: string; children?: React.ReactNode; still?: boolean }
export declare function EventLine(props: EventLineProps): React.ReactElement;

export interface BreadcrumbProps { items: Array<{ label: string; provider?: Provider }>; backLabel?: string }
export declare function Breadcrumb(props: BreadcrumbProps): React.ReactElement;

/** The one composer. Recipient identity comes from `provider` and the placeholder. */
export interface ComposerProps { provider: Provider; model?: string; effort?: string; level?: 1 | 2 | 3 | 4 | 5; fast?: boolean; placeholder?: string; value?: string; running?: boolean; permission?: string; files?: Array<{ name: string; icon?: IconProps['name'] }>; modelOpen?: boolean; permOpen?: boolean; focused?: boolean; pending?: boolean; recipient?: string }
export declare function Composer(props: ComposerProps): React.ReactElement;

export interface PermissionControlProps { value?: string; open?: boolean; icon?: IconProps['name'] }
export declare function PermissionControl(props: PermissionControlProps): React.ReactElement;

/** Lightning, model, quieter effort, one chevron. No provider logo. */
export interface ModelControlProps { provider: Provider; model?: string; effort?: string; level?: 1 | 2 | 3 | 4 | 5; fast?: boolean; open?: boolean; pending?: boolean }
export declare function ModelControl(props: ModelControlProps): React.ReactElement;

export interface PickerModel { name: string; detail?: string; selected?: boolean; unavailable?: boolean; levels?: string[]; effort?: string; details?: string[]; fastAvailable?: boolean }
/** Model picker: current model row, thick effort slider, fast mode. The model row opens the model list (ChatGPT models, Claude models). Clickable. */
export interface ModelPickerProps { groups: Array<{ provider: Provider; label?: string; note?: string; models: PickerModel[] }>; view?: 'effort' | 'models'; levels?: string[]; effort?: string; details?: string[]; fixedEffort?: string; fast?: boolean; fastAvailable?: boolean; fastDetail?: string; lockedProvider?: Provider; lockedNote?: string; note?: string; still?: boolean; provider?: Provider }
export declare function ModelPicker(props: ModelPickerProps): React.ReactElement;

/** Stepped reasoning control. Levels come from the selected engine and model. `size="lg"` is the thick segmented slider. */
export interface EffortSliderProps { provider: Provider; levels: string[]; value: string; size?: 'md' | 'lg'; onChange?: (level: string, index: number) => void; still?: boolean }
export declare function EffortSlider(props: EffortSliderProps): React.ReactElement;

export interface ChangeStripProps { status?: string; statusState?: GlyphState; provider?: Provider; added?: number; removed?: number; expanded?: boolean; title?: string; note?: string; openLabel?: string; files?: Array<{ path: string; added?: number; removed?: number; state?: string }>; still?: boolean }
export declare function ChangeStrip(props: ChangeStripProps): React.ReactElement;

export interface ApprovalCardProps { provider: Provider; kind?: 'command' | 'edit' | 'question'; title?: string; meta?: string; command?: string; reason?: string; primaryLabel?: string; secondaryLabel?: string; denyLabel?: string; options?: string[] }
export declare function ApprovalCard(props: ApprovalCardProps): React.ReactElement;

/** Dotted sphere drawn on a 2D canvas; the live mark while an agent works. */
export interface ThinkingOrbProps { provider: Provider; state?: 'thinking' | 'reading' | 'tool' | 'waiting'; size?: number; still?: boolean; label?: string }
export declare function ThinkingOrb(props: ThinkingOrbProps): React.ReactElement;

/** The quiet live line at the end of the transcript while an agent works. */
export interface WorkingIndicatorProps { provider: Provider; state?: 'thinking' | 'reading' | 'tool' | 'waiting'; activity?: string; summary?: string; trail?: Array<string | { text: string }>; time?: string; size?: number; still?: boolean }
export declare function WorkingIndicator(props: WorkingIndicatorProps): React.ReactElement;

/** The selected agent's plan in the right panel, under Delegated work. */
export interface TodoListProps { provider: Provider; agent?: string; task?: string; title?: string; steps: Array<{ state: PlanStepProps['state']; title: string; detail?: string }>; empty?: string }
export declare function TodoList(props: TodoListProps): React.ReactElement;

/** A task in Delegated work: assignment first, then provider and state, then the latest activity. */
export interface TaskRowProps { title: string; provider: Provider; model?: string; state?: 'working' | 'waiting' | 'done' | 'queued' | 'failed' | 'coordinating' | 'revising' | 'stopped'; stateLabel?: string; activity?: string; selected?: boolean; child?: boolean; attention?: boolean; still?: boolean }
export declare function TaskRow(props: TaskRowProps): React.ReactElement;

export interface MenuItem { type?: 'item' | 'section' | 'divider' | 'usage' | 'note'; label?: string; detail?: string; icon?: IconProps['name']; provider?: Provider; end?: string; kbd?: string | string[]; tag?: string; check?: boolean; danger?: boolean; disabled?: boolean; active?: boolean; [key: string]: unknown }
export interface MenuProps { title?: string; items: MenuItem[]; width?: number }
export declare function Menu(props: MenuProps): React.ReactElement;

export interface EngineCardProps { provider: Provider; name?: string; status?: 'ready' | 'signin' | 'missing' | 'unsupported' | 'checking'; lines?: string[]; text?: string; actions?: Array<{ label: string; variant?: ButtonProps['variant']; icon?: IconProps['name']; iconRight?: IconProps['name'] }>; still?: boolean; children?: React.ReactNode }
export declare function EngineCard(props: EngineCardProps): React.ReactElement;

declare global {
  interface Window {
    Bukno: {
      Icon: typeof Icon; ProviderMark: typeof ProviderMark; StatusGlyph: typeof StatusGlyph; StateChip: typeof StateChip;
      Button: typeof Button; IconButton: typeof IconButton; Kbd: typeof Kbd; Switch: typeof Switch;
      SectionLabel: typeof SectionLabel; ChatRow: typeof ChatRow; ProjectRow: typeof ProjectRow; UsageMeter: typeof UsageMeter; ProfileRow: typeof ProfileRow;
      UserMessage: typeof UserMessage; AgentTurn: typeof AgentTurn; PlanStep: typeof PlanStep; ActivityLine: typeof ActivityLine; CodeBlock: typeof CodeBlock; Notice: typeof Notice; TaskBrief: typeof TaskBrief; EventLine: typeof EventLine; Breadcrumb: typeof Breadcrumb;
      Composer: typeof Composer; PermissionControl: typeof PermissionControl; ModelControl: typeof ModelControl; ModelPicker: typeof ModelPicker; EffortSlider: typeof EffortSlider;
      ChangeStrip: typeof ChangeStrip; ApprovalCard: typeof ApprovalCard; ThinkingOrb: typeof ThinkingOrb; WorkingIndicator: typeof WorkingIndicator; TaskRow: typeof TaskRow; TodoList: typeof TodoList; Menu: typeof Menu; EngineCard: typeof EngineCard;
      iconNames: string[];
    };
  }
}
