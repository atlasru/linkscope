// One-shot patch for GitHub Actions: fail closed if the inspected baseline changes.
import {readFileSync,writeFileSync} from 'node:fs';

function patch(path, edits) {
  let text = readFileSync(path, 'utf8');
  for (const [before, after] of edits) {
    const count = text.split(before).length - 1;
    if (count !== 1) throw new Error(`${path}: expected exactly one match, found ${count}: ${before.slice(0,100)}`);
    text = text.replace(before, after);
  }
  writeFileSync(path, text);
}

patch('ui/renderer.js', [
  ['in float active;', 'in float isActive;'],
  ['if(active<.5)', 'if(isActive<.5)'],
  ["this.attrib(p,'active',this.activeBuffer,1)", "this.attrib(p,'isActive',this.activeBuffer,1)"],
]);

const fallbackDraw = `if(renderer.disabled){
  ctx.strokeStyle='rgba(86,129,182,.27)';ctx.lineWidth=1;ctx.beginPath();let drawn=0;
  for(const edge of model.edges){if(drawn++>=5000)break;const a=model.nodes[edge.a],b=model.nodes[edge.b];if(!a||!b||a.timestamp<renderer.timeRange[0]||a.timestamp>renderer.timeRange[1]||b.timestamp<renderer.timeRange[0]||b.timestamp>renderer.timeRange[1])continue;const[x1,y1]=renderer.screen(edge.a),[x2,y2]=renderer.screen(edge.b);ctx.moveTo(x1,y1);ctx.lineTo(x2,y2)}ctx.stroke();
  for(let i=0;i<Math.min(model.nodes.length,3000);i++){const n=model.nodes[i];if(n.timestamp<renderer.timeRange[0]||n.timestamp>renderer.timeRange[1])continue;const[x,y]=renderer.screen(i);ctx.fillStyle=i===selected?'#fce8a6':'#84aaed';ctx.beginPath();ctx.arc(x,y,i===selected?6:3,0,Math.PI*2);ctx.fill()}
 }
 `;

patch('ui/app.js', [
  ["import {Renderer} from './renderer.js';", "import {Renderer} from './renderer.js';\nimport {FallbackRenderer} from './fallback-renderer.js';"],
  ["}catch(e){toast(e.message);$('status').textContent='WebGL2 недоступен';throw e}", "}catch(e){const reason=String(e.message||e);renderer=new FallbackRenderer($('graph'),reason);toast(`Граф недоступен: ${reason}`);$('status').textContent='Граф недоступен · безопасный режим';$('layout').disabled=true;$('layout').title='GPU layout requires WebGL2';log(`Renderer startup failed: ${reason}`,true)}"],
  ["$('graph-note').textContent=demo?'Синтетические данные · без сетевых запросов':'WebGL2 · локальный граф';", "$('graph-note').textContent=renderer.disabled?'GPU недоступен · безопасный режим':demo?'Синтетические данные · без сетевых запросов':'WebGL2 · локальный граф';"],
  ["ctx.scale(d,d);if(!model.nodes.length)return;", "ctx.scale(d,d);if(!model.nodes.length)return;\n "+fallbackDraw],
  ["})();\n// Read-only handles for reproducible renderer tests; no privileged methods exposed.", "})();\nif(renderer.disabled){$('status').textContent='Граф недоступен · безопасный режим';$('graph-note').textContent='GPU недоступен · безопасный режим'}\n// Read-only handles for reproducible renderer tests; no privileged methods exposed."],
]);

patch('.github/workflows/check.yml', [
  ['      - run: node --test tests/model.test.mjs', '      - run: node --test tests/model.test.mjs tests/webgl-init.test.mjs'],
  ["  desktop:\n", `  browser-smoke:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '22'
      - run: npm install --no-save playwright
      - run: npx playwright install --with-deps chromium
      - name: Test normal WebGL2 startup and UI safe mode
        run: |
          python3 -m http.server 8765 --bind 127.0.0.1 --directory ui >/tmp/linkscope-http.log 2>&1 &
          sleep 2
          node tests/webgl-smoke.mjs
  desktop:
`],
]);
console.log('WebGL2 shader, guarded UI startup, Canvas2D overlay safe mode and CI checks patched');
