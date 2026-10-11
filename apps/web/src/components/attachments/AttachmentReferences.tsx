/** References without trusted metadata are visible text, never fake links. */
export function AttachmentReferences({ ids }: { ids: readonly string[] }) {
  if (!ids.length) return null;
  return (
    <ul
      className="studio-message-attachments"
      aria-label="Saved file references"
    >
      {ids.map((id, index) => (
        <li className="studio-message-attachment" key={id}>
          Saved attachment {index + 1}
        </li>
      ))}
    </ul>
  );
}
