order+='|second';
document.getElementById('status').textContent='Carregamento real: '+order;
var missing=document.createElement('script');
missing.src='missing.js';
missing.onerror=function(){document.getElementById('errors').textContent='Erro de carga entregue ao callback.';};
document.head.appendChild(missing);
