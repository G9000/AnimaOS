import { randomBytes } from 'node:crypto';

const ownerOverrides = new Set([
  'ANIMA_LOCAL_ADMIN_TOKEN',
  'ANIMA_LOCAL_ADMIN_TOKEN_FILE',
  'ANIMAOS_TEST_OWNER_TOKEN',
  'ANIMAOS_RS_API_KEY',
  'ANIMA_ALLOWED_UI_ORIGINS',
  'ANIMA_SUPERVISOR_URL',
  'ANIMA_SUPERVISOR_TOKEN',
  'ANIMA_SUPERVISOR_TOKEN_FILE',
  'ANIMAOS_RS_HOST',
  'ANIMAOS_ALLOW_HOST_EXECUTION',
  'ANIMAOS_EXECUTION_PROFILES_FILE',
]);

/** Each Cargo invocation uses a synthetic owner identity, never the operator's. */
export function rustDaemonTestEnvironment(
  source: NodeJS.ProcessEnv,
): NodeJS.ProcessEnv {
  const isolated = { ...source };
  for (const key of Object.keys(isolated)) {
    if (ownerOverrides.has(key.toUpperCase())) delete isolated[key];
  }
  const token = randomBytes(32).toString('hex');
  isolated.ANIMAOS_TEST_OWNER_TOKEN = token;
  isolated.ANIMA_LOCAL_ADMIN_TOKEN = token;
  return isolated;
}
