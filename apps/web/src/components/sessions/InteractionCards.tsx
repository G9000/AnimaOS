import { useState } from 'react';
import type {
  ClarificationAnswer,
  ClarificationAnswerInput,
  ClarificationRequest,
  SideQuestionAnswer,
} from '@animaOS-SWARM/sdk';
import { revealInvisible } from '../../lib/skills';

export interface ClarificationCardProps {
  request: ClarificationRequest;
  onAnswer: (
    request: ClarificationRequest,
    input: ClarificationAnswerInput,
  ) => Promise<unknown>;
}
const shown = (text: string) => revealInvisible(text).text;

export function ClarificationCard(props: ClarificationCardProps) {
  return (
    <ClarificationForm
      key={`${props.request.id}:${props.request.revision}`}
      {...props}
    />
  );
}

function ClarificationForm({ request, onAnswer }: ClarificationCardProps) {
  const [answers, setAnswers] = useState<Record<string, ClarificationAnswer>>(
    {},
  );
  const [saving, setSaving] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = request.status === 'pending' && !submitted;
  const update = (questionId: string, patch: Partial<ClarificationAnswer>) =>
    setAnswers((current) => ({
      ...current,
      [questionId]: {
        ...(current[questionId] ?? { questionId, choiceIds: [], text: null }),
        ...patch,
      },
    }));
  return (
    <section
      className="approval-card rounded-xl border border-line bg-panel-2 p-4 text-sm"
      aria-label="Question from Anima"
    >
      <p className="mb-2 font-medium">
        {pending
          ? 'Anima needs your input'
          : request.status === 'cancelled'
            ? 'Question cancelled'
            : 'Answers saved'}
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!pending || saving) return;
          setSaving(true);
          setError(null);
          const input = {
            revision: request.revision,
            answers: request.questions.map((question) => {
              const answer = answers[question.id];
              return {
                questionId: question.id,
                choiceIds: answer?.choiceIds ?? [],
                text: answer?.text?.trim() || null,
              };
            }),
          };
          void onAnswer(request, input)
            .then(() => setSubmitted(true))
            .catch((caught) =>
              setError(
                caught instanceof Error
                  ? caught.message
                  : 'The answer could not be saved. Try again.',
              ),
            )
            .finally(() => setSaving(false));
        }}
      >
        {request.questions.map((question) => {
          const value = answers[question.id];
          const saved = request.response?.answers.find(
            (answer) => answer.questionId === question.id,
          );
          return (
            <fieldset
              key={question.id}
              disabled={!pending || saving}
              className="mb-3 min-w-0"
            >
              <legend className="mb-2 font-medium">
                {shown(question.question)}
              </legend>
              {pending ? (
                <>
                  {question.choices.map((choice, index) => (
                    <label key={choice.id} className="mb-1 flex gap-2">
                      <input
                        type={question.multiSelect ? 'checkbox' : 'radio'}
                        name={`${request.id}:${question.id}`}
                        checked={value?.choiceIds.includes(choice.id) ?? false}
                        onChange={(event) =>
                          update(question.id, {
                            choiceIds: question.multiSelect
                              ? event.target.checked
                                ? [...(value?.choiceIds ?? []), choice.id]
                                : (value?.choiceIds ?? []).filter(
                                    (id) => id !== choice.id,
                                  )
                              : [choice.id],
                            ...(!question.multiSelect ? { text: null } : {}),
                          })
                        }
                      />
                      <span>
                        {shown(choice.label)}
                        {index === 0 && question.choices.length > 1
                          ? ' (Recommended)'
                          : ''}
                      </span>
                    </label>
                  ))}
                  <label className="mt-2 block">
                    <span>
                      {question.choices.length ? 'Other answer' : 'Your answer'}
                    </span>
                    <textarea
                      aria-label={`${question.choices.length ? 'Other answer' : 'Your answer'}: ${question.question}`}
                      rows={2}
                      maxLength={4096}
                      className="mt-1 block w-full rounded border border-line bg-panel p-2"
                      value={value?.text ?? ''}
                      onChange={(event) =>
                        update(question.id, {
                          text: event.target.value,
                          ...(!question.multiSelect ? { choiceIds: [] } : {}),
                        })
                      }
                    />
                  </label>
                </>
              ) : (
                <p className="whitespace-pre-wrap text-ink-2">
                  {saved
                    ? shown(
                        [
                          ...saved.choiceIds.map(
                            (id) =>
                              question.choices.find(
                                (choice) => choice.id === id,
                              )?.label ?? id,
                          ),
                          saved.text,
                        ]
                          .filter(Boolean)
                          .join(', '),
                      ) || 'Skipped'
                    : 'No answer submitted'}
                </p>
              )}
            </fieldset>
          );
        })}
        {pending && (
          <button
            type="submit"
            disabled={saving}
            className="rounded border border-line px-3 py-2"
          >
            {saving ? 'Saving…' : 'Submit answers'}
          </button>
        )}
        {error && (
          <p role="alert" className="mt-2 text-danger">
            {error}
          </p>
        )}
      </form>
    </section>
  );
}

export function SideQuestionCard({
  question,
  answer,
  error,
  pending,
}: {
  question: string;
  answer?: SideQuestionAnswer | null;
  error?: string | null;
  pending?: boolean;
}) {
  return (
    <section
      aria-label="Side question"
      className="rounded-xl border border-line bg-panel-2 p-4 text-sm"
    >
      <p className="font-medium">Side question: {shown(question)}</p>
      <p className="mt-1 text-xs text-ink-3">
        The main task continues. This answer uses a conversation snapshot.
      </p>
      {pending && (
        <p role="status" className="mt-2">
          Answering…
        </p>
      )}
      {answer && (
        <p className="mt-2 whitespace-pre-wrap">{shown(answer.answer)}</p>
      )}
      {error && (
        <p role="alert" className="mt-2 text-danger">
          {error}
        </p>
      )}
    </section>
  );
}
