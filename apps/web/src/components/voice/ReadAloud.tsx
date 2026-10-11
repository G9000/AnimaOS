import type { ReadAloudController } from '../../hooks/useReadAloud';

export interface ReadAloudProps {
  messageId: string;
  text: string;
  controller: ReadAloudController;
  disabled?: boolean;
}
export function ReadAloud({
  messageId,
  text,
  controller,
  disabled = false,
}: ReadAloudProps) {
  if (!controller.supported || !text.trim()) return null;
  const speaking = controller.speakingMessageId === messageId;
  return (
    <button
      type="button"
      disabled={disabled}
      aria-pressed={speaking}
      onClick={() => controller.read(messageId, text)}
      className="studio-read-aloud"
    >
      {speaking ? 'Stop reading' : 'Read aloud'}
    </button>
  );
}
