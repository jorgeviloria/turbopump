# Dimensiones principales de un rodete de bomba centrífuga

Implementación: [`crates/tp-design/src/pump/`](../../crates/tp-design/src/pump/)

El cálculo usa teoría de línea de corriente 1D: el flujo se representa con velocidades
medias en tres secciones del rodete:

- la boca de entrada (índice 1);
- el borde de ataque, en la cubierta, la línea media y el cubo;
- la salida (índice 2).

Todas las magnitudes están en SI. Los ángulos se miden desde la dirección
circunferencial, la convención usual en bombas.

**Fuente principal:** J. F. Gülich, *Centrifugal Pumps*, 2.ª ed., Springer 2010. Cada
correlación de esta nota se comprobó ecuación por ecuación contra esa edición; los números
de ecuación y de página se refieren a ella. Las demás ediciones numeran distinto.

## 1. Velocidad específica y tipo de bomba

$n_q$ se calcula **por etapa y por boca**
([velocidad-especifica.md](velocidad-especifica.md)). En un proyecto multietapa la
altura se reparte por igual entre los rodetes.

El tipo de bomba decide qué correlaciones de rendimiento se aplican:

| Tipo | Condición | Válida hasta |
|---|---|---|
| Una etapa, simple aspiración | 1 rodete, simple aspiración | $n_q \le 100$ |
| Multietapa | más de un rodete | $n_q \le 60$ |
| Doble aspiración | 1 rodete de doble aspiración | $n_q \le 50$ |

Todas las correlaciones de rendimiento valen para $Q \ge 0{,}005$ m³/s. Fuera de estos
rangos el programa avisa.

## 2. Rendimientos (tabla 3.9)

Con $Q$ el caudal de la bomba, $Q_{ref} = 1$ m³/s y $r = Q_{ref}/Q$:

**Exponente**, con $a = 1$ si $Q \le 1$ m³/s y $a = 0{,}5$ si es mayor:

$$m = b \cdot a \cdot r^{0,15}\,(45/n_q)^{0,06}$$

- $b = 0{,}1$ para el rendimiento total;
- $b = 0{,}08$ para el hidráulico.

**Rendimiento total:**

| Tipo | Fórmula | Ec. |
|---|---|---|
| Una etapa | $1 - 0{,}095\,r^m - 0{,}3\,[0{,}35 - \log(n_q/23)]^2\,r^{0,05}$ | 3.9.1 |
| Multietapa | $1 - 0{,}116\,r^m - 0{,}4\,[0{,}26 - \log(n_q/25)]^2$ | 3.9.2 |
| Doble aspiración | $1 - 0{,}095\,r^m - 0{,}35\,[0{,}35 - \log(n_q/17{,}7)]^2\,r^{0,05}$ | 3.9.4 |

**Rendimiento hidráulico:**

| Tipo | Fórmula | Ec. |
|---|---|---|
| Una etapa | $1 - 0{,}055\,r^m - 0{,}2\,[0{,}26 - \log(n_q/25)]^2\,r^{0,1}$ | 3.9.6 |
| Multietapa | $1 - 0{,}065\,r^m - 0{,}23\,[0{,}3 - \log(n_q/23)]^2\,r^{0,05}$ | 3.9.8 |

La tabla no trae rendimiento hidráulico para doble aspiración. En ese caso usamos la
ecuación de una etapa con el caudal de una boca; es una decisión propia.

**Tolerancia:** el informe muestra la dispersión de estas correlaciones,
$\Delta\eta = \pm 0{,}2\,(1 - \eta)$ (ec. 3.9.10).

**Rendimientos derivados:**

- $\eta_v = 0{,}97$ es un valor típico. Más adelante se calculará con las fugas por los
  anillos de desgaste (cap. 3.6).
- $\eta_m = \eta / (\eta_h \eta_v)$ incluye el rozamiento de disco.
- El caudal por el rodete es $Q_{La} = Q/\eta_v$ y la altura teórica, $H_{th} = H/\eta_h$.

## 3. Eje (ec. 7.1.2)

$$d_{eje} = \left(\frac{16\,P}{\pi\,\omega\,\tau_{adm}}\right)^{1/3}$$

- El libro usa la **potencia máxima** del rango de operación. Nosotros usamos la del punto
  de diseño de toda la máquina, así que conviene dejar margen en $\tau_{adm}$ o fijar el
  diámetro a mano.
- $\tau_{adm}$ vale 25 MPa por defecto: es un valor preliminar típico para acero.
- El diámetro de cubo en la boca es, por defecto, igual al del eje.

## 4. Diámetro de salida (ec. 3.26 y 7.1.3)

$$\psi_{opt} = 1{,}21\,e^{-0{,}77\,n_q/100}, \qquad u_2 = \sqrt{2 g H/\psi}, \qquad d_2 = 2u_2/\omega$$

Si el usuario fija $d_2$, ψ se recalcula a partir de él.

## 5. Boca de entrada y ancho de salida

**Diámetro de boca** (tabla 7.1, ec. 7.1.4): $d_1 = f_{d1} \cdot d_{1,min}$, donde
$d_{1,min}$ minimiza la velocidad relativa en la cubierta con entrada sin prerrotación:

$$d_{1,min}^2 = d_n^2 + 2\left(\frac{4 Q_{La}}{\pi \omega}\right)^{2/3}$$

Es la forma exacta de la ecuación 7.1.4: la del libro está escrita con coeficientes
redondeados y da 72,0 mm frente a nuestros 72,3 mm.

**Factor $f_{d1}$** (tabla 7.1): baja linealmente entre $n_q = 15$ y $40$ y se mantiene
constante fuera de ese rango.

| Tipo de rodete | $n_q \le 15$ | $n_q \ge 40$ |
|---|---|---|
| Normal (por defecto) | 1,15 | 1,05 |
| De aspiración (`--set inlet=suction`) | 1,25 | 1,15 |

**Ancho de salida** (ec. 7.1):

$$b_2/d_2 = 0{,}017 + 0{,}262\,x - 0{,}08\,x^2 + 0{,}0093\,x^3, \qquad x = n_q/100$$

En un rodete de doble aspiración, $b_2$ total = $2 \cdot (b_2/d_2)(n_q) \cdot d_2$. Se
avisa si el ancho de salida de cada lado no queda por debajo del de entrada,
$b_1 = (d_1 - d_n)/2$ (p. 345).

## 6. Álabes y triángulos de velocidad

**Bloqueo** (ecs. 3.1.7 y 3.2.3), con $\lambda = 90°$:

$$\tau = \left(1 - \frac{z\,e}{\pi d \sin\beta_B \sin\lambda}\right)^{-1}$$

**Entrada** (ec. 7.1.7):

$$\beta_{1B} = \arctan\frac{\tau_1 c_{m1}}{u_1} + i$$

La incidencia $i$ vale 0° por defecto. El libro recomienda de 0° a 4°, y se avisa si se
sale de ese rango.

**Salida** (ecs. 3.2.4-3.2.7). El ángulo $\beta_{2B}$ se obtiene con el método de Brent de

$$\frac{c_{u2}}{u_2} = \gamma - \frac{c_{m2}\,\tau_2}{u_2 \tan\beta_{2B}}$$

con el deslizamiento

$$\gamma = f_1\left(1 - \frac{\sqrt{\sin\beta_{2B}}}{z^{0,7}}\right) k_w, \qquad \varepsilon_{lim} = e^{-8{,}16 \sin\beta_{2B}/z}$$

- $f_1 = 0{,}98$ en rodetes radiales.
- $f_1 = 1{,}02 + 1{,}2\cdot10^{-3}(n_q - 50)$ en rodetes semiaxiales.
- $k_w$ corrige los canales cortos cuando $d_{1m}/d_2 > \varepsilon_{lim}$.

**Número de álabes.** Gülich desaconseja las fórmulas empíricas y da criterios
(pp. 342-343):

- de 5 a 7 álabes para $10 < n_q < 120$;
- nunca 8 o más, por la estabilidad de la curva Q-H;
- no menos de 5 si la altura por etapa supera 100 m.

El valor automático usa la estimación de Pfleiderer, $z = 6{,}5\,(d_2 + d_{1m})/(d_2 - d_{1m})\,\sin\beta_m$,
redondeada y **limitada a 5-7 dentro de $10 < n_q < 120$**. Fuera de ese rango se
admiten de 3 a 12. Después se comprueba con el criterio de carga del álabe
([geometria-rodete.md](geometria-rodete.md)). A mano se aceptan de 3 a 24 álabes.

**Subida de altura hasta caudal cero.** Lobanoff y Ross (*Centrifugal Pumps: Design and
Application*, 2.ª ed., cap. 3) eligen el número de álabes y su ángulo de salida según la
forma de curva que se busca: con menos álabes y ángulos más planos la curva sube más
desde el punto óptimo hasta caudal cero. Sus figuras 3-2 y 3-13 se ajustaron con
fórmulas propias:

$$\frac{H_0 - H}{H} = 0{,}063\; z^{-0{,}632}\; n_q^{\,0{,}995 - 0{,}052\,z}$$

$$eta_{2B,	ext{estándar}} = 5{,}8° + 4{,}233°\,z - 0{,}1667°\,z^2 \qquad (3 \le z \le 8)$$

La primera se leyó en la fig. 3-2 entre $N_s$ = 200 y 3500 (unidades de EE. UU.,
$N_s = 51{,}65\,n_q$), es decir $n_q$ ≈ 4-68, y queda a menos de 2 puntos de la gráfica
para 3 a 8 álabes. La segunda da 17° con 3 álabes, 25° con 6 y 29° con 8.

- **Subida automática:** no se impone; el resultado muestra la subida que corresponde al
  número de álabes y al $\beta_{2B}$ del diseño. Un ángulo distinto del estándar entra
  promediando $z$ con el número de álabes cuyo ángulo estándar es ese $\beta_{2B}$
  (interpolación propia). La curva de prestaciones usa esta subida para la altura a
  válvula cerrada ([prestaciones.md](prestaciones.md)).
- **Subida manual:** se toma el $z$ de 3 a 8 cuya curva queda más cerca del objetivo, y
  $\beta_{2B}$ estándar para ese $z$, salvo que $z$ o $\beta_{2B}$ sean manuales. Con 7 u 8
  álabes se avisa: en la gráfica esas curvas caen hacia caudal cero, y Gülich desaconseja
  8 o más por la estabilidad.

**Ángulo de salida prescrito.** Normalmente $d_2$ sale de ψ y $\beta_{2B}$ se despeja para
dar la altura. Si $\beta_{2B}$ está fijado (a mano o por la subida de altura) se invierte el
cálculo: se busca con Brent el $d_2$ que cumple

$$u_2\left(\gamma u_2 - \frac{c_{m2}\,\tau_2}{\tan\beta_{2B}}\right) = g H_{th}$$

con $b_2$ y $e$ que dependen de $d_2$. Si $d_2$ también es manual se despeja $b_2$, que es
como Lobanoff mantiene el punto óptimo al quitar álabes (fig. 3-15). Con $d_2$, $b_2$ y
$\beta_{2B}$ manuales a la vez el problema está sobredeterminado y se rechaza. $\beta_{2B} = 90°$
da álabes radiales.

**Área entre álabes a la entrada.** Como dato se calcula
$A_v/A_e = z\,a_1 b_1 / A_e$, con la garganta $a_1 = \pi d_1 \sin\beta_{1B}/z - e$ en la
cubierta y $b_1 = (d_1 - d_n)/2$. La fig. 3-12 de Lobanoff da $0{,}275 + 0{,}00496\,n_q
\pm 0{,}055$, pero corresponde a ojos grandes ($\beta_{1B}$ ≈ 15°-20°). El ojo de $w_1$
mínima de Gülich da $\beta_{1B}$ ≈ 35° y un cociente mayor, así que no se avisa.

**Espesor.** Por defecto, $\max(3\text{ mm};\ 0{,}016\,d_2)$. El libro recomienda
$e/d_2$ = 0,016-0,022, con un mínimo de 3 a 5 mm en piezas fundidas (p. 347).

## 6b. Rodetes semiaxiales (flujo mixto, cap. 7.5)

**Cuándo.** Por encima de $n_q \approx 60$ un rodete semiaxial supera al radial, y rinde
mejor entre $n_q$ = 40 y 160. En automático el tipo pasa a semiaxial cuando
$n_q > 60$; se puede forzar radial o semiaxial.

**Convención.** Como en Gülich, lo que no lleva subíndice es de la línea de corriente
exterior: $d_2 = d_{2a}$, y $\psi$ (ec. 3.26) fija $d_{2a}$. El borde de salida va de
$d_{2i}$ en el cubo a $d_{2a}$ en la cubierta:

$$\varepsilon_s = 90°\left(\frac{n_q}{200}\right)^{0{,}74} \quad\text{(7.12)}, \qquad
\frac{d_{2m}}{d_{2a}} = e^{-0{,}04\,(n_q/30 - 1)} \quad\text{(7.13)}, \qquad
\frac{d_{2i}}{d_{2a}} = \sqrt{2\left(\frac{d_{2m}}{d_{2a}}\right)^2 - 1} \quad\text{(7.14)}$$

$\varepsilon_s$ es el ángulo de la línea exterior con una normal al eje (0° radial, 90°
axial). Por debajo de $n_q$ = 30 la ec. 7.13 daría $d_{2m} > d_{2a}$ y se limita a 1. Tanto
$\varepsilon_s$ como $d_{2i}$ se pueden fijar a mano; con $d_{2i}$ manual,
$d_{2m} = \sqrt{(d_{2a}^2 + d_{2i}^2)/2}$. $\varepsilon_s$ se limita a 80°.

**Triángulo de salida.** Se calcula en la línea exterior, como en el rodete radial, pero
con el área $A_2 = \pi d_{2m} b_2$ (ec. 3.2.2), el ancho $b_2/d_{2a}$ de la ec. 7.1
(fig. 7.2) y el factor del deslizamiento semiaxial $f_1 = 1{,}02 + 1{,}2\cdot10^{-3}(n_q - 50)$
(ec. 3.2.6).

**Rendimientos.** Ecs. 3.9.3 y 3.9.7 de la tabla 3.9, válidas desde $n_q$ = 45.

**Reparto del trabajo.** Vórtice libre, $c_u r$ = cte (tabla D7.2, caso A): todas las líneas
de corriente hacen el mismo trabajo. El $\beta_{2B}$ de cada sección sale de la misma
relación de deslizamiento que la exterior, con $u$ y $c_u$ locales:

$$\tan\beta_{2B}(r) = \frac{\tau\,c_{m2}}{\gamma\,u(r) - c_u(r)}, \qquad c_u(r) = c_{u2a}\,\frac{r_{2a}}{r}$$

Así $\beta_{2B,a} < \beta_{2B,i}$, como pide el libro. Si en el cubo $\gamma u \le c_u$, el
cubo es demasiado pequeño para el vórtice libre y se avisa.

**Longitud del álabe.** Se avisa si el álabe exterior queda por debajo de
$L/d_{2a} \ge 1{,}1\,(\beta_{2B,a}/25°)(n_q/200)^{0{,}4}$ (ec. 7.17), el mínimo para una curva
estable.

**Subida de altura.** Fuera de $n_q$ 4-68 (donde acaba la gráfica de Lobanoff), la subida
mostrada y la altura a caudal cero salen de la ec. 3.28.

## 7. Comprobaciones (p. 346)

| Comprobación | Límite |
|---|---|
| Ángulo de salida $\beta_{2B}$ | 15°-45° (a menudo 20°-27°) |
| Desviación $\delta_2 = \beta_{2B} - \beta_2$ | ≤ 10°-14° (se avisa por encima de 14°) |
| De Haller, $w_2/w_1$ en la cubierta | ≥ 0,7 |
| Ancho de salida | $b_2 < b_1$ |
| Incidencia | 0°-4° |

## 8. Cavitación

$$NPSH_3 \approx \left(\frac{n\sqrt{Q/f_q}}{n_{ss}}\right)^{4/3}, \qquad NPSH_d = \frac{p_{t,entrada} - p_v}{\rho g}$$

$n_{ss} = 200$ por defecto: es un valor típico de rodetes convencionales. Se avisa si
$NPSH_d < NPSH_3$, y también si el margen es menor del 30 %.

## 9. Bomba de referencia

Datos: 100 m³/h, 32 m, 2900 rpm, agua a 20 °C, todos los valores automáticos.

| Magnitud | Valor |
|---|---|
| $n_q$ | 35,9 |
| $\eta_h$ | 0,907 |
| $\eta$ | 0,814 ± 0,037 |
| $P$ | 10,7 kW |
| $d_{eje}$ | 19,3 mm |
| $f_{d1}$ | 1,066 |
| $d_1$ | 77,2 mm |
| $d_2$ | 172,2 mm |
| $b_2$ | 17,4 mm |
| $z$ | 7 |
| $\beta_{2B}$ | 22,0° |
| $\delta_2$ | 8,8° |
| $w_2/w_1$ | 0,99 |
| $NPSH_3$ | 3,2 m |

Las pruebas automáticas comprueban:

- la ecuación de Euler, $u_2 c_{u2} - u_1 c_{u1} = g H_{th}$;
- la continuidad en la salida;
- que $\eta_h \eta_v \eta_m = \eta$;
- el valor de $\eta$ calculado a mano con la ecuación 3.9.1.

## 10. Estado de verificación

| Elemento | Estado |
|---|---|
| ψ_opt, η_h, η, tolerancia, $b_2/d_2$, deslizamiento, bloqueos, $d_1$ y $f_{d1}$, eje, límites de diseño | **verificado** con la 2.ª ed. |
| Número de álabes | Pfleiderer, estándar, limitado al rango de Gülich |
| Semiaxial: ecs. 7.12-7.14, 7.17, 3.9.3, 3.9.7 y $f_1$ (3.2.6) | **verificado** con la 2.ª ed. |
| Subida de altura, $\beta_{2B}$ estándar, $A_v/A_e$ | ajustes propios a las figs. 3-2, 3-13 y 3-12 de Lobanoff y Ross, comprobados contra las gráficas |
| $\eta_v$, $\tau_{adm}$, $n_{ss}$ | valores típicos, no correlaciones publicadas |

El registro está en código, en `tp_design::pump::correlations::CORRELATIONS`.

## 11. Limitaciones

- No hay prerrotación a la entrada. La ecuación 7.1.9 ya tiene el término para cuando se
  añada.
- $\eta_v$ es constante.
- El eje usa la potencia del punto de diseño.
- Hay rodetes radiales y semiaxiales; faltan los axiales.
- La voluta, el difusor y las pérdidas tratan la salida semiaxial como si fuera radial en
  $d_{2a}$ con ancho $b_2$.
