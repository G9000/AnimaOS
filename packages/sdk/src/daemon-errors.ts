export class DaemonHttpError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, body: unknown) {
    const message =
      typeof body === 'object' &&
      body !== null &&
      'error' in body &&
      typeof body.error === 'string'
        ? body.error
        : `Daemon request failed with status ${status}`;

    super(message);
    this.name = 'DaemonHttpError';
    this.status = status;
    this.body = body;
  }
}
