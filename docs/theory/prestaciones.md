# Prestaciones y análisis del flujo

Implementación:

- [`crates/tp-design/src/performance.rs`](../../crates/tp-design/src/performance.rs) (curva característica);
- [`crates/tp-design/src/impeller/potential.rs`](../../crates/tp-design/src/impeller/potential.rs) (flujo potencial meridional);
- [`crates/tp-design/src/impeller/loading.rs`](../../crates/tp-design/src/impeller/loading.rs) (velocidades en las caras del álabe);
- [`crates/tp-math/src/sparse.rs`](../../crates/tp-math/src/sparse.rs) (sistemas dispersos);
- [`crates/tp-design/src/npsh.rs`](../../crates/tp-design/src/npsh.rs) (NPSH₃ frente al caudal);
- [`crates/tp-design/src/volute/loss.rs`](../../crates/tp-design/src/volute/loss.rs) (pérdidas de la voluta);
- [`crates/tp-design/src/radial_thrust.rs`](../../crates/tp-design/src/radial_thrust.rs) (empuje radial);
- [`crates/tp-design/src/axial_thrust.rs`](../../crates/tp-design/src/axial_thrust.rs) (empuje axial);
- [`crates/tp-design/src/operation.rs`](../../crates/tp-design/src/operation.rs) (punto de trabajo);
- [`crates/tp-design/src/losses.rs`](../../crates/tp-design/src/losses.rs) (pérdidas secundarias y del rodete);
- [`crates/tp-design/src/viscous.rs`](../../crates/tp-design/src/viscous.rs) (líquidos viscosos).

Este documento recoge diez análisis de un rodete ya diseñado:

- la **curva característica** prevista: altura, potencia y rendimiento frente al caudal;
- el **flujo potencial** en la sección meridional;
- las **velocidades relativas en las dos caras** de cada sección del álabe;
- el **NPSH₃ necesario** frente al caudal;
- las **pérdidas de la voluta** frente al caudal, que entran en la curva;
- el **empuje radial** sobre el rodete frente al caudal;
- el **empuje axial** sobre el rodete frente al caudal;
- el **punto de trabajo** en la instalación, con otra velocidad o el rodete recortado, y
  el rango de funcionamiento continuo;
- el **rendimiento por suma de pérdidas**;
- la **corrección por viscosidad** para aceites y otros líquidos viscosos.

Las referencias principales son de Gülich, *Centrifugal Pumps*, 2.ª edición: capítulos 3
a 6, 9, 11 y 13. El modelo de la voluta es el de D. Japikse, W. D. Marscher y R. B. Furst,
*Centrifugal Pump Design and Performance*, Concepts ETI 1997, capítulo 4.3. Aquí solo se
citan los números de ecuación; la redacción, las derivaciones y las elecciones propias se
señalan como tales.

## 1. Curva característica

![Curvas de la bomba de referencia](../img/curvas-referencia.jpg)

*Bomba de referencia (100 m³/h, 32 m, 2900 rpm, voluta). De arriba abajo: altura con la
altura teórica de Euler, rendimiento total e hidráulico, y potencia. Las líneas a trazos
son la parte estimada, por debajo del inicio de la recirculación. Los marcadores son el
punto de diseño (círculo) y el de máximo rendimiento (rombo).*

El libro advierte que ni las pérdidas hidráulicas ni la recirculación a carga parcial se
pueden calcular de antemano. Por eso la predicción es empírica y parte del punto de diseño
(cap. 4.3). Turbopump sigue ese esquema en cuatro partes.

### Altura teórica

Sin prerrotación a la entrada, el trabajo del rodete es $g H_{th} = u_2 c_{2u}$. La
componente $c_{2u}$ sale del triángulo de salida con el factor de deslizamiento
$\gamma$ y el bloqueo $\tau_2$ (ec. T3.2.7):

$$c_{2u} = u_2\,\gamma - \frac{c_{2m}\,\tau_2}{\tan\beta_{2B}}, \qquad
c_{2m} = \frac{Q_{La}}{\pi d_2 b_2}$$

$Q_{La}$ es el caudal por el rodete: el de la bomba más las fugas. En un rodete semiaxial
$u_2$ es el de la cubierta ($d_{2a}$) y el área es $\pi d_{2m} b_2$ (ec. 3.2.2). Como primera
aproximación, el deslizamiento no depende del caudal mientras no haya recirculación
(cap. 4.1.1). Con eso, $H_{th}$ es una recta que baja al crecer el caudal (ec. 4.1).

### Rendimiento hidráulico y altura

La estadística de muchos ensayos da el rendimiento hidráulico relativo a su máximo
(ec. 4.14):

$$\frac{\eta_h}{\eta_{h,max}} = 1 - 0{,}6\,(q^* - 0{,}9)^2 - 0{,}25\,(q^* - 0{,}9)^3,
\qquad q^* = \frac{Q}{Q_{opt}}$$

El máximo cae en $q^* = 0{,}9$. $\eta_{h,max}$ se ajusta para que en el punto de diseño
$\eta_h$ sea el del diseño. La altura es $H = \eta_h\,H_{th}$ (ec. T3.3.7).

**Fugas:** por defecto salen del análisis de pérdidas (sección 9): el caudal por los
anillos se calcula a cada altura. Con los rendimientos de la estadística
(`--set losses=statistics`), las juntas dejan pasar un caudal que crece con la raíz de la
presión que soportan (ley de orificio): se escala con $\sqrt{H/H_{opt}}$ desde el valor
de diseño que da $\eta_v$. Esta ley es una elección propia.

### Potencia y rendimiento

La potencia es la de Euler más las pérdidas secundarias:

$$P = \rho\,g\,H_{th}\,Q_{La} + P_{sec}$$

$P_{sec}$ reúne el rozamiento de discos y las pérdidas mecánicas, y apenas depende del
caudal (cap. 4.1.2, ec. T3.5.1). Por defecto sale del análisis de pérdidas (sección 9).
Con la estadística, es lo que la potencia de diseño tiene de más sobre la potencia de
Euler. El rendimiento es $\eta = \rho g H Q / P$.

Con álabes curvados hacia atrás, la potencia de Euler es una parábola en el caudal. Su
máximo cae a la mitad del caudal de altura teórica nula (ecs. 4.4 y 4.5). En la bomba de
referencia ese máximo queda hacia 1,3 $Q_{opt}$.

### Zona de recirculación y válvula cerrada

**Inicio de la recirculación:** la recirculación a la entrada empieza típicamente entre
0,4 y 0,75 $Q_{opt}$, y sube con $n_q$ de 10 a 300 (cap. 5.2). Turbopump interpola en
$\ln n_q$ entre esos extremos. La interpolación es una elección propia; la ec. 5.7b afina
el valor, pero necesita la garganta de entrada y queda pendiente.

**Altura a válvula cerrada:** por debajo de ese caudal las pérdidas no se pueden
calcular. La altura se lleva hasta la de válvula cerrada.

Para $n_q$ = 4-68 la da el rodete: la subida de altura $\Delta h$ de su número de álabes
y su $\beta_{2B}$ (ajuste propio a la fig. 3-2 de Lobanoff y Ross, véase
[dimensiones-principales.md](dimensiones-principales.md)) sobre la altura de la curva en
el caudal de diseño:

$$H_0 = H(Q_{opt})\,(1 + \Delta h)\,k_c, \qquad
\Delta h = f\!\left(n_q,\ \tfrac{z + z_\beta}{2}\right)$$

$z_\beta$ es el número de álabes cuyo ángulo estándar (fig. 3-13) es el $\beta_{2B}$ del
rodete; con el ángulo estándar, $z_\beta = z$ y se recupera la gráfica. Promediar los dos es
una interpolación propia: la gráfica solo da las parejas. Así, quitar álabes o bajar
$\beta_{2B}$ empina la curva. La gráfica no distingue voluta de difusor; con difusor de
álabes se multiplica por $k_c = 1{,}31/1{,}25$, la relación entre las ecs. 3.27 y 3.28.

Fuera de ese rango de $n_q$ se usan las ecs. 3.27 y 3.28, que solo dependen de $n_q$:

$$\psi_0 = 1{,}31\,e^{-0{,}3\,n_q/100} \ \text{(con difusor)}, \qquad
\psi_0 = 1{,}25\,e^{-0{,}3\,n_q/100} \ \text{(con voluta)}, \qquad
H_0 = \psi_0\,\frac{u_2^2}{2g}$$

El colector sale del componente que sigue al rodete. Un difusor sin álabes intercambia
menos cantidad de movimiento con el rodete que uno con álabes (cap. 4.1.4), así que usa el
valor de la voluta.

**Tramo entre válvula cerrada y recirculación:**

- la altura sigue una cúbica que sale plana de $H_0$ y se une a la curva calculada;
- la pendiente de unión se limita (criterio de monotonía de Fritsch y Carlson) para que la
  cúbica no añada un máximo propio;
- si aun así la curva sube en algún tramo, el programa avisa de que puede ser inestable;
- la potencia sigue la tangente de la curva calculada hasta $Q = 0$, o hasta un valor que
  da el usuario.

Todo este tramo se marca como estimación. El libro tampoco da una correlación de la
potencia a válvula cerrada más allá de una figura.

**Caudal máximo:** el cálculo llega a 1,5 $Q_{opt}$, dentro del rango de la estadística de
la ec. 4.14, o hasta que la altura se anula.

### Bomba de referencia

Con los rendimientos de la estadística; el análisis de pérdidas de la sección 9 sube el
rendimiento del diseño a 0,854.

| Punto | Q | H | P | η |
|---|---|---|---|---|
| Válvula cerrada | 0 | 39,8 m | 3,0 kW (estimación) | 0 |
| Inicio de la recirculación | 53,2 m³/h (53 %) | 38,6 m | 7,77 kW | 0,717 |
| 75 % | 75,0 m³/h | 36,6 m | 9,45 kW | 0,791 |
| Máximo rendimiento | 95,9 m³/h (96 %) | 32,9 m | 10,53 kW | 0,815 |
| Diseño | 100,0 m³/h | 32,00 m | 10,68 kW | 0,814 |
| 125 % | 125,0 m³/h | 25,0 m | 11,20 kW | 0,760 |
| 150 % | 150,0 m³/h | 16,4 m | 10,99 kW | 0,609 |

Datos de partida: $\beta_{2B} = 22{,}0°$, $\gamma = 0{,}826$, $\tau_2 = 1{,}116$,
$\varphi_2 = 0{,}116$, $n_q = 35{,}9$ y 7 álabes, que dan $\Delta h$ = 24,5 % y
$\psi_0 = 1{,}142$ (las ecs. 3.27-3.28 darían 1,122).

## 2. Flujo potencial meridional

![Flujo potencial en la bomba de referencia](../img/flujo-potencial.jpg)

*Arriba, las líneas de corriente del flujo potencial (a trazos) frente a las del flujo
cuasi unidimensional del paso meridional (grises). Abajo, $c_m$ a lo largo del cubo y de
la carcasa frente a la velocidad uniforme del cálculo cuasi unidimensional.*

**Ecuación:** el paso meridional del diseño supone $c_m$ uniforme en cada
cuasi-ortogonal. El flujo potencial quita esa hipótesis. En un cuerpo de revolución sin
rozamiento, y sin contar el efecto del giro, el flujo meridional tiene una función de
corriente de Stokes $\psi$ que cumple

$$\frac{\partial}{\partial z}\left(\frac{1}{r}\frac{\partial\psi}{\partial z}\right) +
\frac{\partial}{\partial r}\left(\frac{1}{r}\frac{\partial\psi}{\partial r}\right) = 0,
\qquad c_z = \frac{1}{r}\frac{\partial\psi}{\partial r},\quad
c_r = -\frac{1}{r}\frac{\partial\psi}{\partial z}$$

Entre dos superficies de corriente pasa un caudal igual a $2\pi$ veces su diferencia de
$\psi$.

**Discretización:**

- La malla es estructurada: líneas de la entrada a la salida a fracciones iguales de la
  longitud de cada pared, y nodos repartidos por igual de cubo a carcasa. Cada celda se
  parte en dos triángulos lineales.
- La forma débil $\int (1/r)\,\nabla\psi\cdot\nabla v\,dA = 0$ da una matriz simétrica y
  definida positiva, con $r$ tomado en el centro de cada triángulo. Se resuelve por
  gradiente conjugado con precondicionador diagonal.

**Condiciones de contorno:**

- $\psi = 0$ en el cubo y $\psi = 1$ en la carcasa (en fracción del caudal).
- En la entrada y la salida, la condición natural: el flujo cruza esas líneas en
  perpendicular.
- Para que esas fronteras no deformen el resultado se añaden dos prolongaciones rectas:
  un anillo axial delante del ojo, tan largo como alto es el ojo, y detrás de la salida
  una corona de dos anchos de salida en la que cada pared sigue la dirección con que sale
  (radial, o inclinada $\varepsilon_s$ en un rodete semiaxial). Los resultados se dan solo
  en el canal del rodete.

**Velocidades:** $c_m = (Q/2\pi)\,|\nabla\psi|/r$ en cada triángulo, promediado por área
en los nodos. Las líneas de corriente son las curvas de $\psi$ constante.

**Validación:** contra dos soluciones exactas, con un error por debajo del 1 %:

- en un anillo axial, $c_z = Q/(\pi(r_s^2 - r_h^2))$ uniforme y
  $\psi = (r^2 - r_h^2)/(r_s^2 - r_h^2)$;
- entre dos discos paralelos, $c_r = Q/(2\pi r b)$.

**Bomba de referencia:** malla de 81 × 21 nodos en el canal; 186 iteraciones.

| Fracción de pared | $c_m$ cubo | $c_m$ carcasa | Cuasi 1D |
|---|---|---|---|
| 0 (entrada) | 5,13 m/s | 7,70 m/s | 6,53 m/s |
| 0,125 | 4,22 m/s | 8,45 m/s | 6,35 m/s |
| 0,25 | 4,07 m/s | 7,32 m/s | 5,89 m/s |
| 0,5 | 4,05 m/s | 5,26 m/s | 4,76 m/s |
| 0,75 | 3,51 m/s | 4,10 m/s | 3,81 m/s |
| 1 (salida) | 2,89 m/s | 3,17 m/s | 3,04 m/s |

En el codo de axial a radial la carcasa es la pared interior y el flujo va más rápido por
ella, hasta un 40 % por encima del valor medio. Hacia la salida el perfil se aplana y la
media coincide con la continuidad.

## 3. Velocidades en las caras del álabe

**Velocidad media:** en cada sección se supone que el flujo sigue a los álabes, sin
deslizamiento. $c_m$ es la de la línea de corriente de la sección en cada estación: la
del flujo potencial por defecto, o la uniforme de las cuasi-ortogonales con el cálculo
cuasi-1D. La velocidad relativa media es
$w = c_m\,\tau / \sin\beta_B$, con el bloqueo local $\tau$ del espesor.

**Diferencia entre caras (derivación propia):** un tramo de álabe de longitud $ds$, sobre
la superficie de corriente, que eleva $r c_u$ en $d(r c_u)$ lleva la circulación
$2\pi\,d(r c_u)/z$. Esa circulación es la integral de la velocidad alrededor del tramo,
$(w_{SS} - w_{PS})\,ds$. Por tanto:

$$\Delta w = w_{SS} - w_{PS} = \frac{2\pi}{z}\,\frac{d(r c_u)}{ds}, \qquad
w_{SS,PS} = w \pm \frac{\Delta w}{2}$$

**Giro:** $c_u = u - c_m / \tan\beta_B$ se toma de la línea media, sin el bloqueo. El
espesor, que crece desde cero en un borde redondeado, acelera el flujo pero no lo
desvía; meterlo en $c_u$ daría una carga falsa en el borde de ataque.

**Comprobación:** al integrar $\Delta w$ a lo largo del álabe se recupera $2\pi/z$ veces el
cambio de $r c_u$ entre bordes.

**Aviso:** el programa avisa si la cara de presión llega a velocidad negativa, porque el
flujo se despegaría allí. No se avisa en el último 5 % del álabe: junto al borde de
salida el flujo ya se separa del álabe con deslizamiento (cap. 3.3), y la estimación, que
lo mantiene pegado, no vale ahí (elección propia). Sobre el flujo potencial, $c_m$ cae
deprisa en la cubierta al llegar a la salida y daría allí un pico de carga ficticio.

**Bomba de referencia:** $u_2 = 26{,}15$ m/s, flujo potencial y envolvimiento común
(valores por defecto).

| Span | $w_{SS,max}/u_2$ | $w_{PS,min}/u_2$ | $w$ medio en el borde de ataque | $w$ medio en el de salida |
|---|---|---|---|---|
| 0 (cubo) | 0,590 | −0,062 | 0,226 | 0,329 |
| 0,5 | 0,666 | 0,019 | 0,396 | 0,342 |
| 1 (carcasa) | 0,795 | −0,017 (en el último 1 %) | 0,508 | 0,361 |

En el cubo la cara de presión baja de cero hacia la mitad del álabe ($\bar m$ = 0,42), y
se avisa. Lo provoca el envolvimiento común: el cubo, que por sí solo envolvería 145°,
sube β a mitad del álabe para quedarse en 120°, y gira el flujo más deprisa. Con el
cálculo cuasi-1D y cada sección con su envolvimiento, el mínimo era +0,043.

## 4. NPSH₃ frente al caudal

El procedimiento es el del capítulo 6.3.3 de Gülich. La entrada se supone sin
prerrotación.

**Ecuación base** (ec. 6.10):

$$NPSH_3 = \lambda_c\,\frac{c_{1m}^2}{2g} + \lambda_w\,\frac{w_1^2}{2g}$$

- $c_{1m}$ es la velocidad meridional en la boca y $w_1$ la relativa en la línea de
  corriente exterior del borde de ataque.
- $\lambda_c = 1{,}1$ con entrada axial (de 1,2 a 1,35 con carcasa de entrada radial).
- En el punto de diseño, $\lambda_{w,3} = 0{,}3\,(\tan\beta_{1a})^{0,57}$, con
  $\beta_{1a}$ el ángulo del flujo en la línea exterior (Fig. 6.20, dispersión de
  ±25 %). Los datos de la figura van de 10° a 25°; fuera de ese rango el informe dice
  que el valor está extrapolado.

**Tres caudales característicos:**

| Caudal | Qué pasa | Ec. |
|---|---|---|
| $Q_{SF}$ | entrada sin choque en la línea exterior: por encima, la incidencia es negativa y la cavitación pasa a la cara de presión | 6.17 |
| $Q_W$ | la velocidad relativa $w_1$ iguala a la de la garganta de entrada $w_{1q}$: por encima, el flujo acelera hacia la garganta | 6.18 |
| $Q_c$ | la velocidad media en la garganta del colector iguala a $c_{2u}$ | 6.19 |

La garganta de entrada de los álabes se estima con las dimensiones principales: el paso
por el seno del ángulo del álabe menos el espesor, en el diámetro medio, por el ancho de
la boca. Es una estimación propia; la geometría real la afinará.

**La curva:**

- El codo es $Q_{sa}$, el menor entre $Q_c$ y $Q_{PS} = (Q_{SF} + Q_W)/2$ (ec. 6.20).
- Por debajo de $Q_{sa}$ se usa la ec. 6.10 con
  $\lambda_w = \lambda_{w,opt}\,(\tan\beta_1/\tan\beta_{1,opt})^{0,57}$ (ec. 6.21).
- Por encima sube como $NPSH_{sa}\,(Q/Q_{sa})^x$ (ec. 6.22). El libro da $x$ de 2 a 3;
  usamos 2,5 por defecto.
- Los valores por debajo del inicio de la recirculación se marcan como estimación, igual
  que la altura.

El NPSH disponible sale de la presión total de entrada y la presión de vapor. Si la
curva lo alcanza dentro del rango calculado, el programa avisa del caudal.

El NPSH₃ del paso de dimensiones principales sigue siendo el de la velocidad específica
de aspiración $n_{ss}$: es el objetivo de diseño. El de la curva sale de la geometría de
la entrada.

**Comprobación con el libro de Japikse:** en su ejemplo del capítulo 7.5, la ec. 6.10
con $\lambda_c = 1$, $\lambda_w = 0{,}20$ y las velocidades en la punta de la entrada
($c_{1m} = 13{,}343$ ft/s, $w_1 = 55{,}88$ ft/s) da exactamente el NPSH que publica,
12,47 ft. Es una prueba automática.

**Bomba de referencia** (con su voluta):

| Magnitud | Valor |
|---|---|
| $\beta_{1a}$, $\lambda_{w,3}$ | 28,8° (por encima de los datos), 0,215 |
| $NPSH_3$ en el diseño | 4,36 m |
| $Q_{SF}$ / $Q_W$ / $Q_c$ | 100 % / 113 % / 125 % |
| $Q_{sa}$ | 107 % |
| NPSH disponible | 10,1 m, que la curva alcanza al 143 % |

El valor de diseño es mayor que el de $n_{ss} = 200$ (3,2 m) porque el ojo de la
referencia es pequeño: el ángulo $\beta_{1a}$ queda por encima de los de la figura 6.20.

## 5. Pérdidas de la voluta frente al caudal

**Modelo de la espiral** (Japikse, ecs. 4.4 a 4.8). En la entrada de la voluta
(estación 5) se definen el parámetro de giro $\lambda = c_{\theta 5}/c_{m5}$ y la
relación de áreas $AR = A_7/A_5$, la garganta frente a $2\pi r_5 b_5$:

- la energía cinética meridional que entra en la espiral se pierde:
  $K_m = 1/(1 + \lambda^2)$ (ec. 4.7);
- si el giro tiene que frenar hasta la velocidad de la garganta $c_7$, se pierde como en
  un ensanchamiento brusco: $K_\theta = (\lambda - 1/AR)^2/(1 + \lambda^2)$ (ec. 4.8);
  si acelera, no se pierde nada más;
- la recuperación estática es $C_p = 1 - 1/(AR^2(1 + \lambda^2)) - K$ (ec. 4.6).

**Lectura propia del vórtice libre.** El libro deja que el giro se frene como vórtice
libre de $r_5$ a un radio $r_6$ antes del ensanchamiento. Lo leemos como el giro en el
centro de la garganta, $\lambda_6 = \lambda\,r_5/r_6$, que es el que se compara con
$c_7$. (El libro escribe en su lugar un factor $(r_5/r_6)^2$ sobre la pérdida.) Con
$r_6 = r_5$ las dos formas son la ec. 4.8. Con nuestra lectura, una espiral dimensionada
por momento angular tiene su menor pérdida en su caudal de diseño, como dice Gülich en el
capítulo 4.2.

**Entrada y garganta.** La entrada es el círculo del corte de agua con el ancho $b_3$; el
giro llega a ella como vórtice libre desde $r_2$, también a través de un difusor sin
álabes. La garganta y su radio de centro salen del diseño de la voluta.

**Tobera.** Pierde $(c_{p,id} - c_p)\,c_7^2/2g$, con $c_{p,id} = 1 - 1/A_R^2$
(Gülich ec. 1.40) y una recuperación real limitada por la mejor que alcanza un difusor
de esa longitud, $c_{p,opt} = 0{,}36\,(L/R_1)^{0,26}$ (Gülich ec. 7.31). Es la misma
forma que la ec. T3.8.22 de Gülich.

**Rozamiento en las paredes de la espiral.** El modelo de Japikse no lo incluye. Se
calcula aparte con la ec. T3.8.21 de Gülich,
$Z = \sum (c_f + 0{,}0015)\,c^3\,\Delta A/(2g\,Q)$:

- Como pide Gülich (cap. 3.8), dentro de cada sección $c_u\,r = K$ es constante, y $K$
  cambia a lo largo de la espiral: $K = q\,Q/\int (b/r)\,dr$, con $q$ la fracción del
  caudal recogida hasta el ángulo $\varphi$ (caudal repartido por igual a lo largo de
  la espiral). Un elemento de pared $\Delta A = r\,d\varphi\,d\ell$ da entonces
  $\sum c^3 \Delta A = Q^3 \int (q/\int (b/r)\,dr)^3 \oint r^{-2}\,d\ell\,d\varphi$,
  con la integral de contorno por las paredes mojadas (sin la abertura hacia el
  rodete) y 360 pasos en $\varphi$.
- $c_f$ es el de una placa plana (ec. T3.8.3) tan larga como la espiral, a la velocidad
  de la garganta en el diseño, con agua fría y 16 µm de rugosidad (elección propia, la
  de los canales del rodete).
- En una voluta doble se suman las dos volutas parciales, como indica el libro, y el
  canal exterior, cerrado por todos los lados, justo por fuera del nervio y con su
  velocidad media (estimación propia).

Solo entra en el análisis de pérdidas del punto de diseño (sección 9); la curva de la
voluta frente al caudal sigue siendo la de Japikse.

**Cómo entra en la curva (reparto propio).** La estadística de la ec. 4.14 describe
bombas con volutas bien dimensionadas. Por eso:

1. En el punto de diseño, las pérdidas hidráulicas $(1 - \eta_h)H_{th}$ se reparten entre
   una **voluta ajustada equivalente** (la misma con la garganta que da
   $\lambda_6 \cdot AR = 1$) y el resto de la etapa.
2. El resto conserva la forma estadística de la ec. 4.14.
3. La voluta real añade sus propias pérdidas a cada caudal:
   $H = H_{th} - h_{voluta}(Q) - h_{resto}(Q)$.

Una voluta ajustada deja la curva casi igual. Una garganta pequeña pierde altura en el
diseño, adelanta el máximo rendimiento y pierde mucha altura en sobrecarga. Una garganta
grande retrasa el máximo y pierde menos en la tobera; Gülich recomienda algo de exceso
de garganta cuando la sigue un difusor con pérdidas (cap. 4.2).

**Comprobación con el libro de Japikse** (ejemplo del capítulo 7.5, tras un difusor sin
álabes, $\lambda \cdot AR = 1$). Son pruebas automáticas:

| Magnitud | Modelo | Libro |
|---|---|---|
| $AR$ | 0,2393 | 0,24 |
| $K$ de la espiral | 0,0542 | 0,054 |
| $C_p$ de la espiral | 0,000 | 0,000 |
| $K$ de espiral y tobera | 0,234 | 0,234 |
| $C_p$ de espiral y tobera | 0,356 | 0,356 |

Además, la etapa diseñada por Turbopump para el mismo punto (5000 gpm, 84,05 ft,
1000 rpm, con difusor sin álabes y voluta) sale ajustada como la del libro:
$\lambda = 4{,}23$ frente a 4,18 y $K = 0{,}053$ frente a 0,054.

**Bomba de referencia:**

| Magnitud | Valor |
|---|---|
| $\lambda$, $AR$, $\lambda_6 \cdot AR$ | 7,11, 0,187, 0,99 |
| Pérdida en el diseño | 1,28 m, el 39 % de las pérdidas hidráulicas |
| Espiral / tobera | 0,14 m ($K$ = 0,019) / 1,14 m |
| Menor pérdida | al 99 % del caudal de diseño |
| Rozamiento en las paredes | 0,81 m |
| Altura en el diseño | 31,97 m con la estadística (la voluta está ajustada al 99 %); 32,32 m con el análisis de pérdidas, que deja menos fuga |
| Máximo rendimiento | 93 % del caudal, η = 0,817 |

## 6. Empuje radial

Fuera del punto de diseño, la presión alrededor del rodete deja de ser uniforme en la
voluta y aparece una fuerza radial que cargan el eje y los cojinetes. Turbopump sigue la
tabla 9.4 de Gülich (cap. 9.3):

$$F_R = k_R\,\rho\,g\,H\,d_2\,b_{2,tot} \qquad \text{(ec. 9.6)}$$

$b_{2,tot}$ es el ancho de salida con las dos tapas del rodete. Su espesor por defecto es
el del modelo CAD, $\max(3\ \text{mm};\ 0{,}02\,d_2)$. $q^*$ se refiere al punto de
diseño del colector.

**Voluta simple:**

- $k_{R0}$ a válvula cerrada, del ajuste de la figura 9.27 frente a
  $n_{q,tot} = n_q\sqrt{f_q}$. La curva 1 es para holguras normales (ec. 3.12) y la
  curva 2 para holguras dobles (`--set clearances=wide`).
- $k_{R,opt}$ en $q^* = 1$, del ajuste de la figura 9.28.
- Entre los dos, $k_R = (k_{R0} - k_{R,opt})(1 - q^{*2}) + k_{R,opt}$.
- Por encima de $q^* = 1$, $k_R = k_{R,opt}\,q^{*2}$, la alternativa que da la tabla al
  factor 0,09.

**Voluta doble:**

- $k_R = F_{Dsp}\,k_{R0}$ con $F_{Dsp} = 1{,}75 - 0{,}0083\,\varepsilon_{sp}$
  (fig. 9.29), casi constante hasta $q^* \approx 1{,}1$.
- Con nervios de menos de 180°, la fórmula de interpolación de la tabla acerca los
  valores en $q^* = 0{,}5$ y 1 a los de la voluta simple.
- Por encima de 1,1 el libro dice que la fuerza puede subir mucho; seguimos con $q^{*2}$
  (elección propia).

**Difusor con álabes:** la tabla da rangos ($k_{R0}$ = 0,02 a 0,09 y $k_{R,opt}$ = 0,01
a 0,06). Usamos el centro de cada uno con la ley de la voluta simple (elección propia).

**Para el diseño mecánico** se suma la parte variable $k_{R,dyn}$ (centro de los rangos
de la tabla) y se toma $k_{R,tot} \ge 0{,}15$ (notas 12 y 13 de la tabla).

Un difusor sin álabes toma el empuje de la voluta que lo sigue. Sin voluta detrás no hay
datos.

**Bomba de referencia** (voluta simple, $n_q$ = 35,9):

| Punto | $k_R$ | $F_R$ |
|---|---|---|
| Diseño | 0,023 | 30 N |
| Válvula cerrada | 0,304 | 487 N |
| Para el diseño mecánico | — | 640 N |

## 7. Empuje axial

Las presiones sobre los dos discos del rodete no se compensan: el trasero tiene más área
mojada, desde el sello del eje $d_D$ hasta $d_2$, que el delantero, desde el anillo de
desgaste $d_{sp}$. La fuerza resultante empuja el rodete hacia la aspiración y la carga
el cojinete axial. Turbopump sigue la tabla 9.2 de Gülich (cap. 9.2), con el signo del
libro: **positivo hacia la aspiración**.

**Subida de presión en el rodete** (ec. T9.2.2, con $\eta_{h,La}/\eta_h = 1$ como
aconseja el libro y $H/\eta_h = H_{th}$):

$$\Delta p_{La} = \rho\,g\,H\left(1 - \frac{g\,H_{th}}{2\,u_2^2}\right)$$

**Giro del líquido en las cámaras laterales** (tabla 9.1):

- Sin fuga, $k_o = 0{,}5$: es la ec. T9.1.3 para cámaras abiertas al colector
  ($r_w = r_2$, $t_{ax} = 0$) con el mismo rozamiento en la carcasa y en el rodete. Se
  puede cambiar.
- La fuga hacia dentro por el anillo delantero acelera el giro:
  $k = k_o\,e^{300\,\varphi_{sp}(r_2/r_{sp} - 1)}$ (ec. T9.1.4 con $b = 1$).
- $\varphi_{sp} = 5{,}5\cdot10^{-4}\,\psi_{opt}^{1,5}(n_q/20)^{0,4}$ para las holguras de
  la ec. 3.12 (ec. T9.1.2a). Con `clearances=wide` se dobla (elección propia, como en el
  empuje radial).
- Fuera del diseño la fuga se escala con $\sqrt{H/H_{opt}}$, porque sigue a la
  diferencia de presión en el anillo (elección propia).

**Fuerzas** (tabla 9.2):

- Sobre cada disco, ec. T9.2.3, con $x = d_{sp}/d_2$ delante y $x = d_D/d_2$ detrás:
  $$F = \pi r_2^2\left\{(1 - x^2)\,\Delta p_{La} - \frac{\rho}{4}\,u_2^2\,\bar k^2\,(1 - x^2)^2\right\}$$
  Es la integral de la presión de la ec. T9.1.7 entre $x$ y $r_2$; una prueba lo
  comprueba.
- $F_{Hy} = F_{RS} - F_{FS}$ (ecs. T9.2.6 y T9.2.8, con un $k$ distinto en cada disco).
- Cantidad de movimiento $F_I = \rho\,Q_{La}\,c_{1m}$, con $\varepsilon_2 = 90°$ en
  rodetes radiales (ec. T9.2.9). $Q_{La}$ incluye la fuga del anillo delantero.
- Fuerza sobre el eje $F_w = \frac{\pi}{4}\,d_D^2\,(p_{amb} - p_1)$ (ec. T9.2.10), con
  $p_1$ la presión estática en el ojo y la atmósfera estándar fuera del sello.
- Total $F_{ax} = F_{Hy} - F_I + F_w$ (ec. T9.2.11), sin la fuerza del acoplamiento.

**Con agujeros de equilibrado** y un anillo trasero de diámetro $d_{s2}$ (por defecto
$d_{sp}$):

- Queda una fracción de la fuerza sin agujeros, de 0,1 a 0,2 según la ec. T9.2.14;
  tomamos 0,15.
- Se suma la fuerza de la ec. T9.2.13 cuando $d_{s2} \ne d_{sp}$. Un anillo trasero
  mayor empuja hacia la impulsión.
- **Tamaño de los agujeros** (cap. 9.2.2): uno por canal, y entre todos al menos cuatro
  veces el área de paso del anillo trasero desgastado al doble de su holgura,
  $\tfrac14 z\,d^2 \ge 4\,d_{s2}\,s$ con $s = 2 s_0$ y $s_0$ de la ec. 3.12. El
  diámetro se redondea hacia arriba a 0,5 mm (elección propia). Los mismos agujeros se
  taladran en el CAD del rodete (`turbopump cad --balance-holes`).

**Rodete de doble aspiración:** se equilibra solo, salvo un resto de signo desconocido,
$F_{ax} = f_{ax}\,\frac{\rho}{2}\,u_2^2\,(d_2^2 - d_{sp}^2)$ (ec. T9.2.15). Tomamos
$f_{ax}$ = 0,015, el centro del rango estacionario de 0,01 a 0,02. La parte variable
puede llegar a 0,02–0,06.

**Diámetros por defecto (elección propia):**

- $d_{sp}$: el ojo $d_1$ más el espesor del disco por los dos lados, el mismo
  $\max(3\ \text{mm};\ 0{,}02\,d_2)$ del modelo CAD.
- $d_D$: el diámetro del cubo.

**Comprobación:** el informe da también la estimación rápida de la ec. T9.2.12,
$(0{,}7\ \text{a}\ 0{,}9)\,\rho\,g\,H\,\frac{\pi}{4}(d_{sp}^2 - d_D^2)$.

**Bomba de referencia** (sin agujeros, $d_{sp}$ = 84,1 mm, $d_D$ = 19,3 mm):

| Parte | Diseño |
|---|---|
| $\Delta p_{La}$ | 235 kPa (0,74 $\rho g H$) |
| $\bar k$ delante / detrás | 0,61 / 0,50 |
| Discos $F_{Hy}$ | 1115 N |
| Cantidad de movimiento $F_I$ | 180 N |
| Eje $F_w$ | 6 N |
| **Total** | **941 N hacia la aspiración** |
| A válvula cerrada | 1101 N |
| Estimación rápida (ec. T9.2.12) | 1162 a 1493 N |

Con agujeros de equilibrado el total baja a unos −9 N, con 7 agujeros de 8,5 mm (8,4 mm
sin redondear) dentro de un anillo trasero de 84,1 mm con 0,184 mm de holgura.

**Avisos:** si sin agujeros la fuerza apunta hacia la impulsión, el programa avisa. Pasa
con presiones de aspiración altas, como en las bombas de circulación de calderas (cap.
9.2.2).

## 8. Punto de trabajo, velocidad, recorte y rango de operación

**Curva de la instalación** (Gülich cap. 11.1, ecs. 11.1 a 11.3):

$$H_A = H_{est} + R\,Q^2, \qquad R = \frac{H_{A,r} - H_{est}}{Q_r^2}$$

- La parábola pasa por un punto dado $(Q_r, H_{A,r})$, por defecto el de diseño.
- La altura estática es 0 si no se da (elección propia): una instalación solo de
  rozamiento.
- El **punto de trabajo** es el corte con la curva de la bomba. Si hay varios, por
  ejemplo con una curva inestable, se toma el de mayor caudal, que es el estable.

**Otra velocidad** (tabla 3.4, primera aproximación con los mismos rendimientos):

- $Q \propto n$, $H \propto n^2$, $P \propto n^3$ y $NPSH \propto n^2$.
- Una instalación sin altura estática es una parábola de semejanza: el punto de trabajo
  se desliza por ella, y una prueba lo comprueba.

**Rodete recortado** (cap. 4.5):

- Cada punto se mueve por la recta que lo une con el origen:
  $Q'/Q = H'/H = (d_2'/d_2)^m$ (ec. 4.15).
- $m$ vale 2 por defecto, la ley que Gülich cita de [B.5]. El libro da de 2 a 3: cerca de
  3 para recortes pequeños con los álabes afilados por la salida, y cerca de 2 por
  encima del 5 %.
- El mejor rendimiento baja $\Delta\eta = \varepsilon\,(1 - d_2'/d_2)$ (ec. 4.19).
  Tomamos $\varepsilon$ = 0,2 en bombas de voluta y 0,45 con difusor, el centro de
  0,15–0,25 y 0,4–0,5. Aplicamos la misma caída a toda la curva (elección propia).
- La potencia sigue a $\rho g Q' H'/\eta'$. El NPSH₃ conserva su valor a cada caudal,
  porque la entrada no cambia (elección propia).
- **Límites** (ec. 4.23):
  - bombas de voluta: $d_2'/d_2 \ge 0{,}8$ hasta $n_q$ = 40, más
    $0{,}0025\,(n_q - 40)$ hasta $n_q$ = 100. Tomamos el extremo bajo de 0,8 a 0,85;
    Gülich aconseja 0,85 si hay poco margen de NPSH;
  - bombas con difusor: no más del 5 % sin ensayos.

**Para pasar por el punto de la instalación:**

- **Recorte:** se busca en la curva completa el punto de la recta que pasa por el
  origen y por $(Q_r, H_{A,r})$. Luego $d_2'/d_2 = (H_{A,r}/H)^{1/m}$ (ec. 4.17).
- **Velocidad:** se busca el corte de la curva con la parábola $H \propto Q^2$ por ese
  punto, y la velocidad sale de la relación de caudales.

**Rango de funcionamiento continuo** (cap. 11.6):

- Gülich lo define donde el rendimiento no baja del 80–85 % del máximo. Tomamos el 80 %
  (ajustable).
- El extremo alto se recorta donde el NPSH₃ alcanza el NPSH disponible.
- La figura 11.15 da bandas frente a $n_q$, pero solo como gráfico; no se digitalizó.

**Bomba de referencia** (100 m³/h, 32 m, 2900 rpm, $d_2$ = 172,2 mm):

| Caso | Punto de trabajo | Rango continuo |
|---|---|---|
| Por defecto (sistema por el punto de diseño) | 100,3 m³/h, 32,17 m, η 85,3 % | 38,0 a 143,3 m³/h |
| $H_{est}$ = 15 m, por 28 m a 100 m³/h | 107,9 m³/h, 30,1 m | — |
| Lo mismo con $d_2'$ = 165 mm | 98,9 m³/h, 27,7 m, η 83,3 % | 35,1 a 132,2 m³/h |
| 2600 rpm | 89,9 m³/h, 25,9 m | 34,1 a 129,4 m³/h |

Para que la instalación de 15 m pase por 28 m a 100 m³/h hace falta $d_2'/d_2$ = 0,963,
o 0,952 de la velocidad con el diámetro completo. Con $d_2'$ = 130 mm (0,755) salta el
aviso de la ec. 4.23.

**Avisos:**

- el recorte por debajo del límite;
- curvas que no se cortan;
- un punto de trabajo fuera del rango continuo;
- un NPSH₃ en el punto de trabajo mayor que el disponible.

## 9. Rendimiento por suma de pérdidas

El rendimiento de la curva sale por defecto de un balance de potencias (Gülich ecs.
T3.5.1 y T3.5.3):

$$P = \frac{\rho\,g\,H\,Q}{\eta_h\,\eta_v} + \sum P_{RR} + P_m$$

- **Rendimiento hidráulico $\eta_h$:** con una voluta detrás del rodete, sale de sumar
  las pérdidas hidráulicas del punto de diseño (tabla 3.8, más abajo). Sin voluta sigue
  siendo el de la estadística (tabla 3.9). Fuera del diseño manda la forma de la ec.
  4.14, con las pérdidas de la voluta de la sección 5.
- **Fugas, rozamiento de disco y pérdidas mecánicas:** se calculan con la geometría del
  rodete (cap. 3.6).

**Fuga por el anillo de entrada** (tabla 3.7 (1)):

- **Geometría:**
  - holgura radial $s = \Delta d/2$ con $\Delta d/d_{sp} = 0{,}004\,(100\ \text{mm}/d_{sp})^{0{,}53}$
    (ec. 3.12), doble con `clearances=wide`;
  - largo $L_{sp} = 0{,}12\,d_{sp}$ (0,10 a 0,14 según el cap. 3.6.2);
  - rugosidad de 4 µm, la de un mecanizado N7 del ejemplo del libro.
  - $d_{sp}$ es el del empuje axial.
- **Altura en el anillo:** la subida estática del rodete menos la caída en la cámara
  lateral que gira, $\Delta H_{sp} = H_p - k^2\,u_2^2/2g\,(1 - d_{sp}^2/d_2^2)$ (ec.
  T3.7.3).
  - $H_p = H\,(1 - g H_{th}/2u_2^2)$ es la misma subida que en el empuje axial, y da el
    $R_G \approx 0{,}75$ que el libro propone para $n_q$ < 40.
  - $k = 0{,}9\,y_{sp}^{0{,}087}$ (ec. T3.7.2).
- **Caudal:**
  - velocidad axial $c_{ax} = \sqrt{2g\,\Delta H/(\zeta_{EA} + \lambda L_{sp}/2s)}$ con
    $\zeta_{EA}$ = 1,1 (ec. T3.7.6);
  - $\lambda$ de las ecs. T3.7.9 a T3.7.14 según el régimen, con el efecto del giro del
    eje, resuelto por iteración;
  - $Q_{sp} = \pi\,d_{sp}\,s\,c_{ax}$.
- Fuera del diseño la fuga se recalcula con la altura de cada punto. Un rodete de doble
  aspiración tiene dos anillos; los agujeros de equilibrado añaden la fuga del anillo
  trasero.

**Rozamiento de disco** (tabla 3.6):

- $P_{RR} = k_{RR}\,\rho\,\omega^3 R^5\{1 - (R_n/R)^5\}$ por cada cara (ec. T3.6.2).
- $k_{RR}$ de la ec. T3.6.3 para discos hidráulicamente lisos, con una holgura axial
  $s_{ax} = 0{,}05\,r_2$ (elección propia; Gülich ve su efecto pequeño).
- La fuga hacia dentro lo reduce con $f_L = e^{-350\,\varphi_{sp}(r_2/r_{sp} - 1)}$ (ec.
  T3.6.7).
- El disco delantero va desde el anillo y el trasero desde el sello del eje.

**Pérdidas mecánicas:** $P_m/P = 0{,}0045\,(Q_{Ref}/Q)^{0{,}4}(n_{Ref}/n)^{0{,}3}$ con
$Q_{Ref}$ = 1 m³/s y $n_{Ref}$ = 1500 rpm (ec. T3.5.6a), constantes con el caudal.

**Viscosidad:** la del líquido del proyecto, que ahora pasa al punto de diseño. Con un
aceite el anillo pasa a régimen laminar y fuga menos, y los discos rozan más.

**Validación:** en el ejemplo del cap. 7.5 de Japikse (r₂ = 8,82 in a 1000 rpm), el
rozamiento de disco es de 1,46 hp. Nuestra fórmula, con las dos caras, da lo mismo dentro
de un 10 %, y una prueba lo comprueba.

**Bomba de referencia:**

| Magnitud | Análisis de pérdidas | Estadística |
|---|---|---|
| Anillo | $d_{sp}$ 84,1 mm, $s$ 0,184 mm, $L$ 10,1 mm | — |
| $\Delta H_{sp}$, $k$, $\lambda$ | 15,2 m, 0,57, 0,063 | — |
| Fuga | 1,80 m³/h, $\eta_v$ = 0,982 | $\eta_v$ = 0,970 |
| Rozamiento de disco | 107 W delante + 148 W detrás | — |
| Pérdidas mecánicas | 165 W | — |
| Secundarias (disco y mecánicas) | 3,9 % de la potencia | 7,4 % ($\eta_m$ = 0,926) |
| Rendimiento en el diseño | **0,856** | 0,814 ± 0,037 |
| Altura en el diseño | 32,32 m | 31,97 m |

La estadística del libro supone más pérdidas secundarias que las calculadas para esta
bomba pequeña. El programa muestra los dos rendimientos, y con `losses=statistics` (o la
casilla *Efficiencies from statistics*) vuelve al anterior. Los anillos dejan menos
fuga que la estadística: el rodete pasa menos caudal y da un poco más de altura.

**Pérdidas hidráulicas** (tabla 3.8, con agua fría). Cada una se pasa a altura con
$Z = \zeta\,u_2^2/2g$:

- **Canales del rodete** (tabla 3.8 (1)):
  - gargantas de cada canal: a la entrada $a_1 = \pi d_{1m} \sin\beta_{1B}/z - e$ por
    $b_1 = (d_1 - d_n)/2$, la misma que usa el NPSH₃; a la salida
    $a_2 = \pi d_2 \sin\beta_{2B}/z - e$ por $b_2$;
  - largo del canal $L_{sch} = (r_2 - r_{1m})/\sin\beta_m$, con $\beta_m$ la media de
    los ángulos de entrada y salida, como una espiral logarítmica (elección propia);
  - velocidad media $w_{av}$ (ec. T3.8.2), $c_f$ de placa plana con 16 µm de rugosidad
    (ec. T3.8.3), diámetro hidráulico (ec. T3.8.4), coeficiente de disipación
    $c_d = (c_f + 0{,}0015)(1{,}1 + 4 b_2^*)$ (ec. T3.8.5) y
    $\zeta_{La,R} = 4 c_d\,(L_{sch}/D_h)(w_{av}/u_2)^2$ (ec. T3.8.6);
  - choque en la garganta de entrada, $\zeta_{La,C} = 0{,}3\,((w_{1m} - w_{1q})/u_2)^2$
    (ecs. T3.8.7 a T3.8.9), con entrada sin giro. Cuando la garganta acelera el flujo
    ($w_{1q} > w_{1m}$) no se cuenta (elección propia).
- **Choque a la salida del rodete:** $\zeta_{LS} = \varphi_2^2\,(\tau_2 - b_2/b_3)^2$
  (ec. T3.8.24), el ensanchamiento de $b_2$ al ancho de entrada de la voluta.
- **Voluta:** el rozamiento en sus paredes (ec. T3.8.21) y la mezcla en la espiral y la
  tobera del modelo de la sección 5.

Entonces $\eta_h = 1 - \sum Z/H_{th}$ en el diseño. Gülich advierte (cap. 3.8) que
estas estimaciones solo valen cerca del punto óptimo y que no evalúan bien las pérdidas
de mezcla. Por eso solo fijan el nivel en el diseño: fuera de él, el resto de las
pérdidas sigue la forma de la ec. 4.14 y la voluta su propio modelo.

**Bomba de referencia** ($H_{th}$ = 35,58 m en el diseño):

| Pérdida | Altura |
|---|---|
| Rozamiento en el rodete ($c_f$ = 0,0057) | 1,05 m |
| Choque a la entrada ($w_{1q}/w_{1m}$ = 0,92) | 0,01 m |
| Choque a la salida | 0,11 m |
| Paredes de la voluta | 0,81 m |
| Espiral y tobera de la voluta | 1,28 m |
| **$\eta_h$** | **0,909** (estadística 0,907) |

La suma queda a 0,2 puntos de la estadística en esta bomba. Una garganta de voluta un
40 % más pequeña baja $\eta_h$, y una prueba lo comprueba. Con voluta doble las paredes
pierden 1,34 m y la espiral 1,54 m, y $\eta_h$ baja a 0,886: Gülich advierte de las
pérdidas añadidas del canal exterior (cap. 7.8.1).

**Desglose del rendimiento.** Con el análisis de pérdidas, el rendimiento del diseño
(con agua) se da también como producto:

$$\eta = \eta_h\,\eta_v\,\eta_{RR}\,\eta_m$$

| Factor | Definición | Fuente |
|---|---|---|
| $\eta_h$ | $H/H_{th}$ | — |
| $\eta_v$ | $Q/(Q + Q_{sp})$, con la fuga a la altura del punto | ec. T3.5.9 |
| $\eta_{RR}$ | $1 - \sum P_{RR}/P_i$, el rozamiento de disco | propia |
| $\eta_m$ | $1 - P_m/P$ | ec. T3.5.6 |
| $\eta_i$ | $\eta/\eta_m$, el rendimiento interno, con $P_i = P - P_m$ | ecs. T3.5.4 y T3.5.5 |

Gülich no define un rendimiento del rozamiento de disco. El nuestro es la parte de la
potencia interna que llega al fluido en los canales. Sale del balance de la ec. T3.5.1,
$\rho g H_{th}(Q + Q_{sp}) = \eta_{RR}\,P_i$, así que el producto es exacto. Otros
programas definen ese factor de otra forma, así que conviene comparar $\eta_v$ y $\eta_m$,
que son las de Gülich. Las fugas entre etapas y del dispositivo de equilibrado de la
ec. T3.5.1 no se modelan.

Con las estadísticas (`losses=statistics`) no hay desglose. $\eta_m = \eta/(\eta_h\eta_v)$
de las dimensiones principales incluye entonces el rozamiento de disco.

En la bomba de referencia:

$$0{,}856 = 0{,}909 \cdot 0{,}982 \cdot 0{,}975 \cdot 0{,}984, \qquad \eta_i = 0{,}870$$

**Claves:** `--set losses=`, `seal_clearance=`, `seal_length=`, `seal_roughness=` y
`axial_gap=`.

## 10. Líquidos viscosos

La curva se prevé siempre para agua fría ($\nu$ = 1 mm²/s), también el análisis de
pérdidas de la sección 9, y después se corrige para el líquido (Gülich cap. 13.1, tabla
13.1). A cada punto le corresponde un punto del líquido:

$$Q_v = f_Q\,Q_w, \qquad H_v = f_H(q^*)\,H_w, \qquad \eta_v = f_\eta\,\eta_w,
\qquad P_v = \frac{\rho g Q_v H_v}{\eta_v}$$

con $f_H(q^*) = 1 - (1 - f_H)\,q^{*0{,}75}$ y $q^* = Q_w/Q_{BEP,w}$ (ec. T13.1.6). A
válvula cerrada la altura no cambia; la potencia sube.

**Factores por análisis de pérdidas** (tabla 3.10, el método que el libro considera más
preciso cuando se conoce la geometría):

- **Rozamiento en los canales:** $c_f$ a $Re = \omega r_2^2/\nu$.
  - El turbulento sale de la ec. T3.10.16 (ec. 1.33) y el laminar de la ec. T3.10.17
    (ec. 1.33a).
  - El libro pasa de uno a otro en $Re_{crit} = 3\cdot10^6/(1 + 10^4\,Tu^{1,7})$.
    Tomamos el mayor de los dos: equivale a una transición temprana por la turbulencia
    de la bomba y evita que el rozamiento baje al pasar a laminar (elección propia).
  - Rugosidad de 16 µm, la clase N9 (elección propia).
- **Rendimiento hidráulico:**
  $f_{\eta h} = 1 - \frac{\zeta_R}{\psi}\left(\frac{c_{f,v}}{c_{f,w}} - 1\right)$ con
  $\frac{\zeta_R}{\psi} = \left(\frac{1}{\eta_h} - 1\right)(0{,}635 - 0{,}0016\,n_q\sqrt{f_q})$
  (ecs. T3.10.18 y T3.10.19), y $f_Q = f_H = f_{\eta h}$ (ec. T3.10.22).
- **Rendimiento total:** $f_\eta$ de la ec. T3.10.20. Usa el rozamiento de disco y las
  pérdidas mecánicas del agua, con el disco escalado por $k_{RR,v}/k_{RR,w}$ de la ec.
  T3.6.3. La fuga no cambia ($f_{\eta vol}$ = 1, como supone la tabla).

**Correlación de comparación** (figs. 13.8 y 13.9, ecs. T13.1.1 a T13.1.4):

- $f_H = Re_{mod}^{-6{,}7/Re_{mod}^{0{,}735}}$ y $f_\eta = Re_{mod}^{-19/Re_{mod}^{0{,}705}}$;
- $Re_{mod} = Re\,\omega_s^{1{,}5} f_q^{0{,}75}$ y $\omega_s = n_q/52{,}9$.

Gülich da una dispersión de ±15 % frente a los ensayos.

**Normalización:** los dos juegos de factores se dividen por su valor con agua, para
que el agua dé exactamente 1 (elección propia). La correlación sola daría 0,987 en
$f_\eta$ con agua. Un líquido a menos de un 5 % de la viscosidad del agua fría se trata
como agua, sin corrección.

**NPSH₃** a caudal constante (ec. 13.12), también normalizado con el agua:
$f_{NPSH} = 1 + \zeta_E\,(c_{f,v}/c_{f,w})\,c_{1m}^2/(2g\,NPSH_3)$, con
$\zeta_E = \lambda_c - 1$.

**Bomba de referencia** (el punto de diseño del agua, 100 m³/h a 32,32 m, pasa a):

| $\nu$ | Punto | η | $f_H$ / $f_\eta$ por pérdidas | Correlación | NPSH₃ × |
|---|---|---|---|---|---|
| 10 mm²/s | 98,91 m³/h, 31,95 m | 0,832 | 0,989 / 0,972 | 0,989 / 0,958 | 1,008 |
| 50 mm²/s | 96,46 m³/h, 31,10 m | 0,788 | 0,965 / 0,921 | 0,964 / 0,871 | 1,027 |
| 200 mm²/s | 92,20 m³/h, 29,63 m | 0,704 | 0,922 / 0,823 | 0,913 / 0,715 | 1,058 |
| 1000 mm²/s | 74,31 m³/h, 23,45 m | 0,416 | 0,743 / 0,486 | 0,780 / 0,419 | 1,171 |

Los dos métodos dan casi la misma altura. En el rendimiento el análisis de pérdidas es
algo más optimista, dentro de la dispersión del libro.

**Dónde:** `--set viscosity=` (admite cSt) o el apartado *Liquid* de la vista *Curves*.
Por defecto se usa la viscosidad del líquido del proyecto. Fuera de 7 < $n_q$ < 50 o por
encima de 4000 mm²/s el programa avisa, porque son los límites de los ensayos de la
tabla 13.1.

## Dónde se usa

- **Interfaz:**
  - vista *Curves*: las cuatro gráficas (altura, rendimiento, potencia y NPSH₃ con el
    NPSH disponible y la pérdida en la voluta) en una cuadrícula de 2×2 que deja libre el
    panel de propiedades, con una leyenda común para los puntos de diseño, de máximo
    rendimiento y de trabajo y las líneas de recirculación y de funcionamiento
    continuo. Encima, una línea con las cifras clave (diseño, máximo rendimiento, punto
    de trabajo, NPSH₃ y empujes); el resto de resultados va plegado en *More results*,
    y los ajustes (inicio de la recirculación, $\psi_0$, caudal máximo, líquido y
    funcionamiento en la instalación) en *Settings*;
  - opción *potential flow* de la vista meridional;
  - cuarta gráfica de la vista *Blade*;
  - secciones nuevas del informe.
- **Línea de comandos:** `turbopump performance`, que escribe la curva también en CSV
  (con el NPSH₃ y la pérdida en la voluta), y `turbopump analysis`, con el flujo
  potencial y las caras del álabe, también en CSV. Los coeficientes del NPSH₃ se fijan
  con `--set npsh_inlet=`, `npsh_blade=` y `npsh_exponent=`, y las holguras de los
  empujes con `--set clearances=wide`. Para el empuje axial están `seal=`,
  `shaft_seal=`, `balance=on`, `rear_seal=` y `k0=`. El CSV añade la columna
  `axial_force_n`.
- **Interfaz:** la vista *Curves* da el empuje axial en el diseño y a válvula cerrada,
  con una casilla para los agujeros de equilibrado en sus ajustes; el informe da sus
  partes. El apartado *Operation in the system* de los ajustes fija la altura estática, el punto de la
  instalación, la velocidad y el diámetro recortado. Dibuja la curva de la instalación,
  el punto de trabajo, la curva a la velocidad y el diámetro de funcionamiento y los
  límites del rango continuo.
- **Línea de comandos, funcionamiento:** `--set static=`, `system_flow=`,
  `system_head=`, `speed=`, `d2=`, `trim_exponent=` y `continuous=`.

## Limitaciones

- **Curva:**
  - el deslizamiento es constante con el caudal; la dependencia que muestra la estadística
    del libro solo está en una figura;
  - la potencia a válvula cerrada es una extrapolación.
- **NPSH₃:** la garganta de entrada de los álabes es una estimación. Con recirculación a
  la entrada el NPSH₃ real deja de bajar o sube, y eso no está modelado.
- **Pérdidas hidráulicas:**
  - la suma de la tabla 3.8 solo se hace con voluta; con difusor sigue la estadística;
  - no se cuentan las pérdidas de la entrada ($\zeta_{EA}$), que el libro considera
    despreciables en bombas de aspiración axial, ni las de mezcla por un perfil de
    velocidades no uniforme, que el libro tampoco evalúa;
  - en la voluta doble, el canal exterior solo pierde por rozamiento (el libro lo
    calcularía además como un codo, tabla 1.4), y el caudal se reparte entre las dos
    volutas según sus ángulos, no según sus resistencias.
- **Voluta:** solo se modela una voluta detrás del rodete, directamente o tras un difusor
  sin álabes. La pérdida de la tobera es una estimación por la longitud. Tras un difusor
  con álabes, su garganta fija el codo del NPSH₃, pero sus pérdidas siguen en la
  estadística.
- **Flujo potencial:** no tiene en cuenta el giro ni el rozamiento, y no se usa todavía
  para construir los álabes; el diseño sigue con el flujo cuasi unidimensional.
- **Velocidades en las caras:** sin deslizamiento, la carga no se anula en el borde de
  salida como en el flujo real.
- **Empuje axial:**
  - el giro de entrada a las cámaras laterales y la recirculación a carga parcial, que
    según Gülich son la mayor incertidumbre, no se modelan;
  - $k$ es un promedio (opción A del libro), no la integración paso a paso de la ec.
    T9.1.9;
  - en bombas multietapa la cifra es la de un rodete; faltan los tambores y discos de
    equilibrado (cap. 9.2.3);
  - faltan los rodetes semiaxiales y axiales (ecs. T9.2.16 a T9.2.18).
- **Pérdidas:**
  - el rendimiento hidráulico sigue en la estadística; las pérdidas por rozamiento y
    mezcla del rodete de la tabla 3.8 no se calculan;
  - discos lisos, anillos lisos sin ranuras ni cámaras, y sin el anillo entre etapas ni
    el disco de equilibrado de las multietapa;
  - el dimensionado del rodete sigue usando la $\eta_v$ de la estadística.
- **Viscosidad:** la corrección es empírica, del orden de ±15 %. No tiene en cuenta el
  calentamiento del líquido, que cambia la viscosidad localmente, ni el arranque con
  líquido frío.
- **Punto de trabajo:**
  - la velocidad no cambia el rendimiento (sin efecto de Reynolds);
  - el recorte aplica la misma caída de rendimiento a toda la curva y deja el NPSH₃ como
    está;
  - no hay bombas en paralelo ni en serie, ni curvas de instalación que no sean
    parábolas;
  - los empujes y el resto del informe siguen en la velocidad y el diámetro de diseño.
- **Pendiente:** el análisis álabe a álabe en dos dimensiones.
