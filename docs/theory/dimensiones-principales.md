# Dimensiones principales de un rodete de bomba centrífuga

Implementación: [`crates/tp-design/src/pump/`](../../crates/tp-design/src/pump/)

El cálculo usa teoría de línea de corriente 1D: el flujo se representa con velocidades
medias en tres secciones del rodete:

- la boca de entrada (índice 1);
- el borde de ataque, en la cubierta, la línea media y el cubo;
- la salida (índice 2).

Todas las magnitudes están en SI. Los ángulos se miden desde la dirección
circunferencial, la convención usual en bombas.

> **Estado:** varias correlaciones están marcadas *pendientes de verificar* (ver §9).
> Los resultados son plausibles y coinciden con tamaños de catálogo, pero falta comprobar
> los coeficientes contra la edición original de cada libro antes de usarlos en una
> máquina real.

## 1. Velocidad específica

$n_q$ se calcula **por etapa y por boca**: con $Q/f_q$ y $H/n_{etapas}$
([velocidad-especifica.md](velocidad-especifica.md)). En un proyecto multietapa la
altura se reparte por igual entre los rodetes.

## 2. Rendimientos

| Magnitud | Valor automático | Fuente |
|---|---|---|
| $\eta_h$ | $1 - 0{,}055\,(Q_{ref}/Q)^m - 0{,}2\,(0{,}26 - \log_{10}(n_q/25))^2 (Q_{ref}/Q)^{0,1}$ | Gülich, tabla 3.9 |
| $\eta$ | $1 - 0{,}095\,(Q_{ref}/Q)^m - 0{,}3\,(0{,}35 - \log_{10}(n_q/23))^2 (Q_{ref}/Q)^{0,05}$ | Gülich, tabla 3.9 |
| $m$ | $0{,}08\,a\,(Q_{ref}/Q)^{0,15}(45/n_q)^{0,06}$, con $a = 1$ si $Q \le 1$ m³/s y $a = 0{,}5$ si es mayor | Gülich, tabla 3.9 |
| $\eta_v$ | 0,97 | valor típico |
| $\eta_m$ | $\eta / (\eta_h \eta_v)$; incluye el rozamiento de disco | derivado |

Con estos rendimientos:

- el caudal por el rodete, con fugas, es $Q_{La} = Q/\eta_v$;
- la altura teórica es $H_{th} = H/\eta_h$.

## 3. Eje

$$T = \frac{\rho g Q H_{máquina}}{\eta\,\omega}, \qquad d_{eje} = \left(\frac{16\,T}{\pi\,\tau_{adm}}\right)^{1/3}$$

- $T$ es el par de toda la máquina en el acoplamiento.
- La fórmula es la de torsión de un eje macizo.
- $\tau_{adm}$ vale 25 MPa por defecto: es un valor preliminar típico para acero.
- El diámetro de cubo en la boca es, por defecto, igual al del eje.

## 4. Diámetro de salida

$$\psi_{opt} = 1{,}21\,e^{-0{,}77\,n_q/100}, \qquad u_2 = \sqrt{\frac{2 g H}{\psi}}, \qquad d_2 = \frac{2 u_2}{\omega}$$

Fuente: Gülich, ec. 3.26 con $f_T = 1$, donde $\psi = 2gH/u_2^2$. Si el usuario fija
$d_2$, ψ se recalcula a partir de él.

## 5. Boca de entrada y ancho de salida

**Diámetro de boca de mínima velocidad relativa.** Sin prerrotación ($c_{u1} = 0$), en la
cubierta:

$$w_1^2 = c_{m1}^2 + u_1^2, \qquad c_{m1} = \frac{4 Q_{La}}{\pi (d_1^2 - d_n^2)}, \qquad u_1 = \frac{\omega d_1}{2}$$

Derivando respecto de $d_1$ e igualando a cero:

$$d_1^2 = d_n^2 + 2\left(\frac{4 Q_{La}}{\pi \omega}\right)^{2/3}$$

Sin cubo, este óptimo da $\tan\beta_1 = 1/\sqrt 2$ ($\beta_1 \approx 35{,}3°$). La
constante 2,9 de la fórmula de Gülich, $d_1 = 2{,}9\,(Q/(n\,k_n \tan\beta_1))^{1/3}$, es
$(240/\pi^2)^{1/3}$: la misma relación escrita con $n$ en rpm. El usuario puede escalar
$d_1$ con un factor mayor que 1 para bajar $c_{m1}$ y mejorar la aspiración.

**Ancho de salida:**

$$b_2/d_2 = 0{,}017 + 0{,}262\,x - 0{,}08\,x^2 + 0{,}0093\,x^3, \qquad x = n_q/100$$

Fuente: Gülich, ec. 7.1. En un rodete de doble aspiración, $b_2$ total = $2 \cdot (b_2/d_2)(n_q) \cdot d_2$.

## 6. Álabes y triángulos de velocidad

**Bloqueo:**

$$\tau = \left(1 - \frac{z\,e}{\pi d \sin\beta_B \sin\lambda}\right)^{-1}$$

$\lambda = 90°$ (álabes perpendiculares a cubo y cubierta).

**Entrada:** $\beta_{1B} = \arctan(\tau_1 c_{m1}/u_1) + i$, resuelto por punto fijo.

**Salida:** con entrada sin prerrotación, $c_{u2} = g H_{th}/u_2$. El ángulo $\beta_{2B}$
se obtiene con el método de Brent de

$$\frac{c_{u2}}{u_2} = \gamma(\beta_{2B}) - \frac{c_{m2}\,\tau_2}{u_2 \tan\beta_{2B}}$$

**Deslizamiento:** Wiesner, con el factor $f_1 = 0{,}98$ de Gülich:

$$\gamma = f_1\left(1 - \frac{\sqrt{\sin\beta_{2B}}}{z^{0,7}}\right) k_w, \qquad \varepsilon_{lim} = e^{-8{,}16 \sin\beta_{2B}/z}$$

$k_w$ corrige los canales cortos cuando $d_{1m}/d_2 > \varepsilon_{lim}$.

**Número de álabes** (Pfleiderer):

$$z = 6{,}5\,\frac{d_2 + d_{1m}}{d_2 - d_{1m}} \sin\frac{\beta_{1B} + \beta_{2B}}{2}$$

Se itera con los ángulos resultantes hasta que $z$ redondeado no cambia, dentro del
intervalo 3–12.

## 7. Cavitación

$$NPSH_3 \approx \left(\frac{n\sqrt{Q/f_q}}{n_{ss}}\right)^{4/3}, \qquad NPSH_d = \frac{p_{t,entrada} - p_v}{\rho g}$$

$n_{ss} = 200$ (rpm, m³/s, m) por defecto: es un valor típico de rodetes convencionales.
Se avisa si $NPSH_d < NPSH_3$, y también si el margen es menor del 30 %.

## 8. Comprobaciones automáticas

- **Euler:** $u_2 c_{u2} - u_1 c_{u1} = g H_{th}$.
- **Continuidad en la salida:** $c_{m2}\,\pi d_2 b_2 = Q_{La}$.
- **Rendimientos:** $\eta_h \eta_v \eta_m = \eta$.
- **Bomba de referencia** (100 m³/h, 32 m, 2900 rpm, agua a 20 °C):

  | Magnitud | Resultado |
  |---|---|
  | $n_q$ | 35,9 |
  | $d_2$ | 172 mm |
  | $d_1$ | 72 mm |
  | $b_2$ | 17,4 mm |
  | $z$ | 7 |
  | $\beta_{2B}$ | 22,0° |
  | $NPSH_3$ | 3,2 m |
  | $\eta$ | 0,835 |

  Estos valores corresponden a una bomba normalizada 65-160.

## 9. Estado de verificación

| Correlación | Estado |
|---|---|
| Relaciones de Euler, continuidad, bloqueo y torsión | física estándar |
| Diámetro de boca de mínima $w_1$ | derivación propia (§5) |
| Número de álabes de Pfleiderer, $k = 6{,}5$ | estándar, reproducida igual en toda la bibliografía |
| ψ_opt, $\eta_h$, $\eta$ y $b_2/d_2$ | **pendiente de verificar** con Gülich, 4.ª ed. |
| Deslizamiento: forma de Wiesner | estándar |
| Deslizamiento: factor $f_1$ de Gülich | **pendiente de verificar** |
| $\eta_v$, $\tau_{adm}$, $n_{ss}$ y espesor de álabe | valores típicos, no correlaciones publicadas |

El registro está en código, en `tp_design::pump::correlations::CORRELATIONS`, para que la
interfaz pueda mostrarlo junto a cada valor automático.

## 10. Limitaciones de esta fase

- El borde de ataque se sitúa en el plano de la boca, así que los ángulos en el cubo
  salen altos. En la Fase 2 su posición vendrá del contorno meridional.
- No hay prerrotación en la entrada.
- $\eta_v$ es constante; más adelante se sustituirá por un cálculo de fugas en los anillos
  de desgaste.
- Solo están implementados los rodetes radiales de bomba. El resto de máquinas llega en la
  Fase 6.

## Referencias

- J. F. Gülich, *Centrifugal Pumps*, 4.ª ed., Springer 2020.
- F. J. Wiesner, "A review of slip factors for centrifugal impellers", *J. Eng. Power* 89
  (1967) 558–572.
- C. Pfleiderer, *Die Kreiselpumpen für Flüssigkeiten und Gase*, 5.ª ed., Springer 1961.
- A. J. Stepanoff, *Centrifugal and Axial Flow Pumps*, 2.ª ed., Wiley 1957.
