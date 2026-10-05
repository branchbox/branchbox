import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
export async function verifyAssets(root,variant){
 const manifest=JSON.parse(await readFile(resolve(root,'assets/manifest.json'),'utf8'));
 if(!manifest.source_revision?.app||!manifest.source_revision?.runtime)throw new Error('Capture app/runtime revision is required in manifest');
 const captures=variant==='launch'?['setup-full','run-command','teardown-plan']:variant==='setup'?['setup-full']:['teardown-plan'];
 const files=['assets/film.html',...manifest.fonts.flatMap(font=>[font.file,font.file.replace('.ttf','-OFL.txt')]),...captures.map(key=>`assets/captures/${key}.png`)];
 for(const file of files){if(!manifest.hashes[file])throw new Error(`No reviewed hash for ${file}`);const got=createHash('sha256').update(await readFile(resolve(root,file))).digest('hex');if(got!==manifest.hashes[file])throw new Error(`Asset changed since review: ${file}; review then refresh-manifest.py before rendering`);}
 for(const key of captures)if(!manifest.provenance[key])throw new Error(`Missing provenance for ${key}`);
 return manifest;
}
