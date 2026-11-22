// Browser module-worker entry. All emulation and message handling live in Rust.
import init, { start_worker } from './phone.js';
await init();
await start_worker();
