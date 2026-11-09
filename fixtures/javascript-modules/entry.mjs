import { name, count, increment } from './exports.mjs';
import * as counter from './counter.mjs';
increment();
import('./counter.mjs').then(namespace => {
    document.getElementById('dynamic').textContent = 'Importação registrada: ' + namespace.count;
});
document.getElementById('result').textContent = name + ': ' + count + '/' + counter.count + ', ' + classicOrder;
document.getElementById('result').textContent += ', origem: ' + import.meta.url;
queueMicrotask(() => {
    document.getElementById('result').textContent += ', microtask concluída';
});
