"""Build a dependency-free, offline browser preview; production CSP is unchanged."""
from pathlib import Path
import json
import re

root = Path(__file__).resolve().parents[1]

def module(name):
    source = (root / 'ui' / name).read_text(encoding='utf-8')
    source = re.sub(r'^import .*?;\n', '', source, flags=re.M)
    return re.sub(r'\bexport (class|function|const) ', r'\1 ', source)

html = (root / 'ui/index.html').read_text(encoding='utf-8')
script = '\n'.join(module(name) for name in ['model.js', 'renderer.js', 'app.js'])
worker = json.dumps(module('community.js'))
script = script.replace("new Worker('./community.js',{type:'module'})",
    'new Worker(URL.createObjectURL(new Blob([' + worker + '],'
    '{type:"text/javascript"})),{type:"module"})')
html = html.replace('<link rel="stylesheet" href="style.css">',
    '<style>' + (root / 'ui/style.css').read_text(encoding='utf-8') + '</style>')
html = html.replace("script-src 'self';", "script-src 'unsafe-inline';")
html = html.replace("connect-src 'self' ipc: http://ipc.localhost;", "connect-src 'none';")
html = html.replace('<script type="module" src="app.js"></script>',
    '<script type="module">' + script + '</script>')
output = root / 'linkscope-preview.html'
output.write_text(html, encoding='utf-8')
print(output)
