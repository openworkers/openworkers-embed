import { json } from '@sveltejs/kit';
import { board } from '../../../lib';

export const GET = async ({ platform }) => json(await board(platform!));
