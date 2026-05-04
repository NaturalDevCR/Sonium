var R=Object.defineProperty;var P=e=>{throw TypeError(e)};var Z=(e,n,i)=>n in e?R(e,n,{enumerable:!0,configurable:!0,writable:!0,value:i}):e[n]=i;var T=(e,n,i)=>Z(e,typeof n!="symbol"?n+"":n,i),p=(e,n,i)=>n.has(e)||P("Cannot "+i);var d=(e,n,i)=>(p(e,n,"read from private field"),i?i.call(e):n.get(e)),_=(e,n,i)=>n.has(e)?P("Cannot add the same private member more than once"):n instanceof WeakSet?n.add(e):n.set(e,i),s=(e,n,i,t)=>(p(e,n,"write to private field"),t?t.call(e,i):n.set(e,i),i);/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */function j(e,n){let i=e.slice(0,n).split(/\r\n|\n|\r/g);return[i.length,i.pop().length+1]}function M(e,n,i){let t=e.split(/\r\n|\n|\r/g),f="",o=(Math.log10(n+1)|0)+1;for(let l=n-1;l<=n+1;l++){let r=t[l-1];r&&(f+=l.toString().padEnd(o," "),f+=":  ",f+=r,f+=`
`,l===n&&(f+=" ".repeat(o+i+2),f+=`^
`))}return f}class c extends Error{constructor(i,t){const[f,o]=j(t.toml,t.ptr),l=M(t.toml,f,o);super(`Invalid TOML document: ${i}

${l}`,t);T(this,"line");T(this,"column");T(this,"codeblock");this.line=f,this.column=o,this.codeblock=l}}/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */function V(e,n){let i=0;for(;e[n-++i]==="\\";);return--i&&i%2}function D(e,n=0,i=e.length){let t=e.indexOf(`
`,n);return e[t-1]==="\r"&&t--,t<=i?t:-1}function S(e,n){for(let i=n;i<e.length;i++){let t=e[i];if(t===`
`)return i;if(t==="\r"&&e[i+1]===`
`)return i+1;if(t<" "&&t!=="	"||t==="")throw new c("control characters are not allowed in comments",{toml:e,ptr:n})}return e.length}function b(e,n,i,t){let f;for(;;){for(;(f=e[n])===" "||f==="	"||!i&&(f===`
`||f==="\r"&&e[n+1]===`
`);)n++;if(t||f!=="#")break;n=S(e,n)}return n}function F(e,n,i,t,f=!1){if(!t)return n=D(e,n),n<0?e.length:n;for(let o=n;o<e.length;o++){let l=e[o];if(l==="#")o=D(e,o);else{if(l===i)return o+1;if(l===t||f&&(l===`
`||l==="\r"&&e[o+1]===`
`))return o}}throw new c("cannot find end of structure",{toml:e,ptr:n})}function k(e,n){let i=e[n],t=i===e[n+1]&&e[n+1]===e[n+2]?e.slice(n,n+3):i;n+=t.length-1;do n=e.indexOf(t,++n);while(n>-1&&i!=="'"&&V(e,n));return n>-1&&(n+=t.length,t.length>1&&(e[n]===i&&n++,e[n]===i&&n++)),n}/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */let G=/^(\d{4}-\d{2}-\d{2})?[T ]?(?:(\d{2}):\d{2}(?::\d{2}(?:\.\d+)?)?)?(Z|[-+]\d{2}:\d{2})?$/i;var m,g,w;const E=class E extends Date{constructor(i){let t=!0,f=!0,o="Z";if(typeof i=="string"){let l=i.match(G);l?(l[1]||(t=!1,i=`0000-01-01T${i}`),f=!!l[2],f&&i[10]===" "&&(i=i.replace(" ","T")),l[2]&&+l[2]>23?i="":(o=l[3]||null,i=i.toUpperCase(),!o&&f&&(i+="Z"))):i=""}super(i);_(this,m,!1);_(this,g,!1);_(this,w,null);isNaN(this.getTime())||(s(this,m,t),s(this,g,f),s(this,w,o))}isDateTime(){return d(this,m)&&d(this,g)}isLocal(){return!d(this,m)||!d(this,g)||!d(this,w)}isDate(){return d(this,m)&&!d(this,g)}isTime(){return d(this,g)&&!d(this,m)}isValid(){return d(this,m)||d(this,g)}toISOString(){let i=super.toISOString();if(this.isDate())return i.slice(0,10);if(this.isTime())return i.slice(11,23);if(d(this,w)===null)return i.slice(0,-1);if(d(this,w)==="Z")return i;let t=+d(this,w).slice(1,3)*60+ +d(this,w).slice(4,6);return t=d(this,w)[0]==="-"?t:-t,new Date(this.getTime()-t*6e4).toISOString().slice(0,-1)+d(this,w)}static wrapAsOffsetDateTime(i,t="Z"){let f=new E(i);return s(f,w,t),f}static wrapAsLocalDateTime(i){let t=new E(i);return s(t,w,null),t}static wrapAsLocalDate(i){let t=new E(i);return s(t,g,!1),s(t,w,null),t}static wrapAsLocalTime(i){let t=new E(i);return s(t,m,!1),s(t,w,null),t}};m=new WeakMap,g=new WeakMap,w=new WeakMap;let A=E;/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */let z=/^((0x[0-9a-fA-F](_?[0-9a-fA-F])*)|(([+-]|0[ob])?\d(_?\d)*))$/,U=/^[+-]?\d(_?\d)*(\.\d(_?\d)*)?([eE][+-]?\d(_?\d)*)?$/,X=/^[+-]?0[0-9_]/,K=/^[0-9a-f]{2,8}$/i,v={b:"\b",t:"	",n:`
`,f:"\f",r:"\r",e:"\x1B",'"':'"',"\\":"\\"};function $(e,n=0,i=e.length){let t=e[n]==="'",f=e[n++]===e[n]&&e[n]===e[n+1];f&&(i-=2,e[n+=2]==="\r"&&n++,e[n]===`
`&&n++);let o=0,l,r="",u=n;for(;n<i-1;){let a=e[n++];if(a===`
`||a==="\r"&&e[n]===`
`){if(!f)throw new c("newlines are not allowed in strings",{toml:e,ptr:n-1})}else if(a<" "&&a!=="	"||a==="")throw new c("control characters are not allowed in strings",{toml:e,ptr:n-1});if(l){if(l=!1,a==="x"||a==="u"||a==="U"){let h=e.slice(n,n+=a==="x"?2:a==="u"?4:8);if(!K.test(h))throw new c("invalid unicode escape",{toml:e,ptr:o});try{r+=String.fromCodePoint(parseInt(h,16))}catch{throw new c("invalid unicode escape",{toml:e,ptr:o})}}else if(f&&(a===`
`||a===" "||a==="	"||a==="\r")){if(n=b(e,n-1,!0),e[n]!==`
`&&e[n]!=="\r")throw new c("invalid escape: only line-ending whitespace may be escaped",{toml:e,ptr:o});n=b(e,n)}else if(a in v)r+=v[a];else throw new c("unrecognized escape sequence",{toml:e,ptr:o});u=n}else!t&&a==="\\"&&(o=n-1,l=!0,r+=e.slice(u,o))}return r+e.slice(u,i-1)}function q(e,n,i,t){if(e==="true")return!0;if(e==="false")return!1;if(e==="-inf")return-1/0;if(e==="inf"||e==="+inf")return 1/0;if(e==="nan"||e==="+nan"||e==="-nan")return NaN;if(e==="-0")return t?0n:0;let f=z.test(e);if(f||U.test(e)){if(X.test(e))throw new c("leading zeroes are not allowed",{toml:n,ptr:i});e=e.replace(/_/g,"");let l=+e;if(isNaN(l))throw new c("invalid number",{toml:n,ptr:i});if(f){if((f=!Number.isSafeInteger(l))&&!t)throw new c("integer value cannot be represented losslessly",{toml:n,ptr:i});(f||t===!0)&&(l=BigInt(e))}return l}const o=new A(e);if(!o.isValid())throw new c("invalid value",{toml:n,ptr:i});return o}/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */function Y(e,n,i){let t=e.slice(n,i),f=t.indexOf("#");return f>-1&&(S(e,f),t=t.slice(0,f)),[t.trimEnd(),f]}function L(e,n,i,t,f){if(t===0)throw new c("document contains excessively nested structures. aborting.",{toml:e,ptr:n});let o=e[n];if(o==="["||o==="{"){let[u,a]=o==="["?Q(e,n,t,f):J(e,n,t,f);if(i){if(a=b(e,a),e[a]===",")a++;else if(e[a]!==i)throw new c("expected comma or end of structure",{toml:e,ptr:a})}return[u,a]}let l;if(o==='"'||o==="'"){l=k(e,n);let u=$(e,n,l);if(i){if(l=b(e,l),e[l]&&e[l]!==","&&e[l]!==i&&e[l]!==`
`&&e[l]!=="\r")throw new c("unexpected character encountered",{toml:e,ptr:l});l+=+(e[l]===",")}return[u,l]}l=F(e,n,",",i);let r=Y(e,n,l-+(e[l-1]===","));if(!r[0])throw new c("incomplete key-value declaration: no value specified",{toml:e,ptr:n});return i&&r[1]>-1&&(l=b(e,n+r[1]),l+=+(e[l]===",")),[q(r[0],e,n,f),l]}/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */let H=/^[a-zA-Z0-9-_]+[ \t]*$/;function I(e,n,i="="){let t=n-1,f=[],o=e.indexOf(i,n);if(o<0)throw new c("incomplete key-value: cannot find end of key",{toml:e,ptr:n});do{let l=e[n=++t];if(l!==" "&&l!=="	")if(l==='"'||l==="'"){if(l===e[n+1]&&l===e[n+2])throw new c("multiline strings are not allowed in keys",{toml:e,ptr:n});let r=k(e,n);if(r<0)throw new c("unfinished string encountered",{toml:e,ptr:n});t=e.indexOf(".",r);let u=e.slice(r,t<0||t>o?o:t),a=D(u);if(a>-1)throw new c("newlines are not allowed in keys",{toml:e,ptr:n+t+a});if(u.trimStart())throw new c("found extra tokens after the string part",{toml:e,ptr:r});if(o<r&&(o=e.indexOf(i,r),o<0))throw new c("incomplete key-value: cannot find end of key",{toml:e,ptr:n});f.push($(e,n,r))}else{t=e.indexOf(".",n);let r=e.slice(n,t<0||t>o?o:t);if(!H.test(r))throw new c("only letter, numbers, dashes and underscores are allowed in keys",{toml:e,ptr:n});f.push(r.trimEnd())}}while(t+1&&t<o);return[f,b(e,o+1,!0,!0)]}function J(e,n,i,t){let f={},o=new Set,l;for(n++;(l=e[n++])!=="}"&&l;){if(l===",")throw new c("expected value, found comma",{toml:e,ptr:n-1});if(l==="#")n=S(e,n);else if(l!==" "&&l!=="	"&&l!==`
`&&l!=="\r"){let r,u=f,a=!1,[h,x]=I(e,n-1);for(let O=0;O<h.length;O++){if(O&&(u=a?u[r]:u[r]={}),r=h[O],(a=Object.hasOwn(u,r))&&(typeof u[r]!="object"||o.has(u[r])))throw new c("trying to redefine an already defined value",{toml:e,ptr:n});!a&&r==="__proto__"&&Object.defineProperty(u,r,{enumerable:!0,configurable:!0,writable:!0})}if(a)throw new c("trying to redefine an already defined value",{toml:e,ptr:n});let[y,C]=L(e,x,"}",i-1,t);o.add(y),u[r]=y,n=C}}if(!l)throw new c("unfinished table encountered",{toml:e,ptr:n});return[f,n]}function Q(e,n,i,t){let f=[],o;for(n++;(o=e[n++])!=="]"&&o;){if(o===",")throw new c("expected value, found comma",{toml:e,ptr:n-1});if(o==="#")n=S(e,n);else if(o!==" "&&o!=="	"&&o!==`
`&&o!=="\r"){let l=L(e,n-1,"]",i-1,t);f.push(l[0]),n=l[1]}}if(!o)throw new c("unfinished array encountered",{toml:e,ptr:n});return[f,n]}/*!
 * Copyright (c) Squirrel Chat et al., All rights reserved.
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice, this
 *    list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the copyright holder nor the names of its contributors
 *    may be used to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */function N(e,n,i,t){var a,h;let f=n,o=i,l,r=!1,u;for(let x=0;x<e.length;x++){if(x){if(f=r?f[l]:f[l]={},o=(u=o[l]).c,t===0&&(u.t===1||u.t===2))return null;if(u.t===2){let y=f.length-1;f=f[y],o=o[y].c}}if(l=e[x],(r=Object.hasOwn(f,l))&&((a=o[l])==null?void 0:a.t)===0&&((h=o[l])!=null&&h.d))return null;r||(l==="__proto__"&&(Object.defineProperty(f,l,{enumerable:!0,configurable:!0,writable:!0}),Object.defineProperty(o,l,{enumerable:!0,configurable:!0,writable:!0})),o[l]={t:x<e.length-1&&t===2?3:t,d:!1,i:0,c:{}})}if(u=o[l],u.t!==t&&!(t===1&&u.t===3)||(t===2&&(u.d||(u.d=!0,f[l]=[]),f[l].push(f={}),u.c[u.i++]=u={t:1,d:!1,i:0,c:{}}),u.d))return null;if(u.d=!0,t===1)f=r?f[l]:f[l]={};else if(t===0&&r)return null;return[l,f,u.c]}function B(e,{maxDepth:n=1e3,integersAsBigInt:i}={}){let t={},f={},o=t,l=f;for(let r=b(e,0);r<e.length;){if(e[r]==="["){let u=e[++r]==="[",a=I(e,r+=+u,"]");if(u){if(e[a[1]-1]!=="]")throw new c("expected end of table declaration",{toml:e,ptr:a[1]-1});a[1]++}let h=N(a[0],t,f,u?2:1);if(!h)throw new c("trying to redefine an already defined table or value",{toml:e,ptr:r});l=h[2],o=h[1],r=a[1]}else{let u=I(e,r),a=N(u[0],o,l,0);if(!a)throw new c("trying to redefine an already defined table or value",{toml:e,ptr:r});let h=L(e,u[1],void 0,n,i);a[1][a[0]]=h[0],r=h[1]}if(r=b(e,r,!0),e[r]&&e[r]!==`
`&&e[r]!=="\r")throw new c("each key-value declaration must be followed by an end-of-line",{toml:e,ptr:r});r=b(e,r)}return t}export{B as p};
