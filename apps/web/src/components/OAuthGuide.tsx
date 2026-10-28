import { useEffect, useRef, type ReactNode } from 'react';
import type { OAuthProvider } from './OAuthSetupLink';

function Link({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noreferrer">
      {children}
    </a>
  );
}

const GUIDES: Record<
  OAuthProvider,
  { title: string; steps: ReactNode[]; tip: string }
> = {
  google: {
    title: 'Get these from Google Cloud',
    steps: [
      <>
        Open the{' '}
        <Link href="https://console.cloud.google.com/projectcreate">
          Google Cloud Console
        </Link>{' '}
        and create a project, or pick one you already have.
      </>,
      <>
        In the{' '}
        <Link href="https://console.cloud.google.com/apis/library">
          API library
        </Link>
        , turn on the Gmail API, the Google Calendar API and the Google Drive
        API.
      </>,
      <>
        Set up the{' '}
        <Link href="https://console.cloud.google.com/apis/credentials/consent">
          consent screen
        </Link>
        . Choose External, give the app a name, and add your own Google account
        as a test user.
      </>,
      <>
        Under{' '}
        <Link href="https://console.cloud.google.com/apis/credentials">
          Credentials
        </Link>
        , choose Create credentials, then OAuth client ID, and set Application
        type to Web application.
      </>,
      <>
        On that Google page, find Authorized redirect URIs and click Add URI.
        Click the empty box that appears and paste with Ctrl+V. The URIs to
        paste are in the Redirect URIs list just below this guide: press Copy
        next to one, paste it, then click Add URI again for the next. When
        every URI in that list is in, click Create.
      </>,
      <>Copy the client ID and client secret, paste them here and save.</>,
    ],
    tip: 'Google can expire connections for apps left in Testing after about a week. Publish the app to avoid reconnecting.',
  },
  microsoft: {
    title: 'Get these from Microsoft Entra',
    steps: [
      <>
        Open the{' '}
        <Link href="https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade">
          Microsoft Entra admin center
        </Link>
        , go to App registrations and choose New registration.
      </>,
      <>
        Name it. For both work and personal accounts, pick “Accounts in any
        organizational directory and personal Microsoft accounts”.
      </>,
      <>
        On the registration form, under Redirect URI, set the type to Web, click
        the box next to it and paste with Ctrl+V. The URI to paste is in the
        Redirect URI list just below this guide: press Copy next to it. Then
        choose Register.
      </>,
      <>
        Open Certificates and secrets and create a client secret. Copy its{' '}
        <strong>Value</strong> straight away; Microsoft shows it only once.
      </>,
      <>
        Copy the Application (client) ID from Overview, paste both here and
        save. Leave Tenant as common unless your organization limits sign-in;
        then use your tenant ID.
      </>,
    ],
    tip: 'If your organization blocks consent, ask an admin to approve the app.',
  },
};

/**
 * Where the owner gets a provider's client ID and secret, step by step.
 * Open by default until the provider is set up, then out of the way.
 */
export function OAuthGuide({
  provider,
  configured,
}: {
  provider: OAuthProvider;
  /** Null until the provider has said where it stands. */
  configured: boolean | null;
}) {
  const element = useRef<HTMLDetailsElement>(null);
  const decided = useRef(false);
  useEffect(() => {
    if (decided.current || configured === null) return;
    decided.current = true;
    if (!configured && element.current) element.current.open = true;
  }, [configured]);
  const guide = GUIDES[provider];
  return (
    <details className="oauth-guide" ref={element}>
      <summary>{guide.title}</summary>
      <ol className="oauth-steps">
        {guide.steps.map((step, index) => (
          <li key={index}>
            <span>{step}</span>
          </li>
        ))}
      </ol>
      <p className="oauth-tip">{guide.tip}</p>
    </details>
  );
}
