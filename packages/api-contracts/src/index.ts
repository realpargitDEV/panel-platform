/**
 * Runtime validation for everything the agent sends and receives.
 *
 * Generated from `crates/api-types` by `pnpm contracts`. The client validates
 * for immediate feedback; the agent revalidates and is the authority. A schema
 * passing here is never taken as permission to skip the server-side check.
 */
export * from './generated.js';
