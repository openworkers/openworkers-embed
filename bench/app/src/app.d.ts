import type { Platform as OpenWorkersPlatform } from '@openworkers/adapter-sveltekit';

declare global {
  namespace App {
    interface Platform extends OpenWorkersPlatform {}
  }
}

export {};
