# Generic email

Open **Connectors → Email** for the selected companion. Enter its email address,
IMAP receiving server and SMTP sending server, ports, usernames and passwords.
Use your provider's app passwords where required. Choose **TLS** for an encrypted
connection from the start, or **STARTTLS** for a required upgrade. Certificates
must validate. Setup verifies both servers without sending a test email.

Passwords are held in the OS credential vault and are cleared from the setup
form when submitted. Configuration and inbox snapshots contain no passwords.
Reconfiguring the account requires entering both passwords again.

Use the inbox and draft form as with Gmail or Outlook. The companion can use
`mail_list_messages` and `mail_create_draft` with provider `email`. A normal draft
sends only after the local owner approves it in Connectors. Closing the browser
does not cancel an accepted send. Only bounded plain text is supported initially;
attachments and rendered HTML are not included.
Receiving processes messages up to 256 KiB and keeps up to 32 KiB of decoded
text. Outgoing draft bodies are limited to 100,000 UTF-8 bytes. Each poll handles
at most 25 new messages; a backlog continues in later polls.

## Automatic replies

Enable **Automatic replies** during setup, enter the allowed sender addresses,
and provide the receiving mail server ID (`authserv-id`) from its
`Authentication-Results` header. Enable this only when the provider confirms it
removes forged authentication headers claiming that ID. The daemon requires
that server's DMARC pass to match the sender's domain. An address in the From
header alone does not establish identity.

Automatic replies apply only to mail arriving after setup. The worker checks
mail about every two minutes while the daemon runs, processes bounded batches,
and keeps a separate conversation for each admitted sender. Historical mail,
self-sent mail, automated/list/bounce mail, ambiguous senders, and messages
without the required authentication result do not start an automatic run.

The daemon chooses the reply recipient from the admitted sender, generates an
immutable draft, and attempts sending once. It adds reply-thread headers and an
automatic-reply marker. The model cannot change that automatic reply's recipient.
Drafts and runs remain available for inspection in the console.

If processing is interrupted, the mailbox identity changes, or a send outcome is
uncertain, automatic replies pause and retain the review reason. Review the run
and draft, and check the provider's Sent folder before creating another reply.
An uncertain send is never automatically retried. Reconnect establishes a fresh
future-mail baseline; unresolved uncertain sends prevent silent reconfiguration.
Disconnect and reconnect as a new connection after resolving the uncertainty
with the provider if necessary. Disconnect prevents stale runs from sending.
