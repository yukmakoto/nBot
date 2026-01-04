import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const srcLogo = path.resolve(__dirname, '..', '..', 'assets', 'nbot_logo.png');
const dstLogo = path.resolve(__dirname, '..', 'public', 'nbot_logo.png');

async function copyIfExists(src, dst) {
  try {
    await fs.copyFile(src, dst);
    return true;
  } catch (e) {
    if (e && typeof e === 'object' && 'code' in e && e.code === 'ENOENT') {
      return false;
    }
    throw e;
  }
}

try {
  const ok = await copyIfExists(srcLogo, dstLogo);
  if (!ok) {
    console.warn(`[webui] logo not found: ${srcLogo}`);
  }
} catch (e) {
  console.warn('[webui] sync-public failed:', e);
}

