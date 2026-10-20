# Prestaciones y análisis del flujo

Implementación:

- [`crates/tp-design/src/performance.rs`](../../crates/tp-design/src/performance.rs) (curva característica);
- [`crates/tp-design/src/impeller/potential.rs`](../../crates/tp-design/src/impeller/potential.rs) (flujo potencial meridional);
- [`crates/tp-design/src/impeller/loading.rs`](../../crates/tp-design/src/impeller/loading.rs) (velocidades en las caras del álabe);
- [`crates/tp-math/src/sparse.rs`](../../crates/tp-math/src/sparse.rs) (sistemas dispersos).

Este documento recoge tres análisis de un rodete ya diseñado:

- la **curva característica** prevista: altura, potencia y rendimiento frente al caudal;
- el **flujo potencial** en la sección meridional;
- las **velocidades relativas en las dos caras** de cada sección del álabe.

Las referencias son de Gülich, *Centrifugal Pumps*, 2.ª edición: capítulos 3, 4 y 5. Aquí
solo se citan los números de ecuación; la redacción, las derivaciones y las elecciones
propias se señalan como tales.

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

$Q_{La}$ es el caudal por el rodete: el de la bomba más las fugas. Como primera
aproximación, el deslizamiento no depende del caudal mientras no haya recirculación
(cap. 4.1.1). Con eso, $H_{th}$ es una recta que baja al crecer el caudal (ec. 4.1).

### Rendimiento hidráulico y altura

La estadística de muchos ensayos da el rendimiento hidráulico relativo a su máximo
(ec. 4.14):

$$\frac{\eta_h}{\eta_{h,max}} = 1 - 0{,}6\,(q^* - 0{,}9)^2 - 0{,}25\,(q^* - 0{,}9)^3,
\qquad q^* = \frac{Q}{Q_{opt}}$$

El máximo cae en $q^* = 0{,}9$. $\eta_{h,max}$ se ajusta para que en el punto de diseño
$\eta_h$ sea el del diseño. La altura es $H = \eta_h\,H_{th}$ (ec. T3.3.7).

**Fugas:** las juntas dejan pasar un caudal que crece con la raíz de la presión que
soportan (ley de orificio), así que se escala con $\sqrt{H/H_{opt}}$. El valor en el punto
de diseño sale de $\eta_v$. Esta ley es una elección propia.

### Potencia y rendimiento

La potencia es la de Euler más las pérdidas secundarias:

$$P = \rho\,g\,H_{th}\,Q_{La} + P_{sec}$$

$P_{sec}$ reúne el rozamiento de discos y las pérdidas mecánicas, y apenas depende del
caudal (cap. 4.1.2, ec. T3.5.1). Se toma como lo que la potencia de diseño tiene de más
sobre la potencia de Euler. El rendimiento es $\eta = \rho g H Q / P$.

Con álabes curvados hacia atrás, la potencia de Euler es una parábola en el caudal. Su
máximo cae a la mitad del caudal de altura teórica nula (ecs. 4.4 y 4.5). En la bomba de
referencia ese máximo queda hacia 1,3 $Q_{opt}$.

### Zona de recirculación y válvula cerrada

**Inicio de la recirculación:** la recirculación a la entrada empieza típicamente entre
0,4 y 0,75 $Q_{opt}$, y sube con $n_q$ de 10 a 300 (cap. 5.2). Turbopump interpola en
$\ln n_q$ entre esos extremos. La interpolación es una elección propia; la ec. 5.7b afina
el valor, pero necesita la garganta de entrada y queda pendiente.

**Altura a válvula cerrada:** por debajo de ese caudal las pérdidas no se pueden
calcular. La altura se lleva hasta la de válvula cerrada, con el coeficiente de las
ecs. 3.27 y 3.28:

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

| Punto | Q | H | P | η |
|---|---|---|---|---|
| Válvula cerrada | 0 | 39,1 m | 3,0 kW (estimación) | 0 |
| Inicio de la recirculación | 53,2 m³/h (53 %) | 38,6 m | 7,77 kW | 0,717 |
| 75 % | 75,0 m³/h | 36,6 m | 9,45 kW | 0,791 |
| Máximo rendimiento | 95,9 m³/h (96 %) | 32,9 m | 10,53 kW | 0,815 |
| Diseño | 100,0 m³/h | 32,00 m | 10,68 kW | 0,814 |
| 125 % | 125,0 m³/h | 25,0 m | 11,20 kW | 0,760 |
| 150 % | 150,0 m³/h | 16,4 m | 10,99 kW | 0,609 |

Datos de partida: $\beta_{2B} = 22{,}0°$, $\gamma = 0{,}826$, $\tau_2 = 1{,}116$,
$\varphi_2 = 0{,}116$, $n_q = 35{,}9$ y $\psi_0 = 1{,}122$.

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
  un anillo axial delante del ojo, tan largo como alto es el ojo, y una corona radial de
  dos anchos de salida detrás de $d_2$. Los resultados se dan solo en el canal del rodete.

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
deslizamiento. $c_m$ se toma del flujo meridional del diseño, interpolada entre las dos
cuasi-ortogonales que rodean cada punto. La velocidad relativa media es
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
flujo se despegaría allí.

**Bomba de referencia:** $u_2 = 26{,}15$ m/s.

| Span | $w_{SS,max}/u_2$ | $w_{PS,min}/u_2$ | $w$ medio en el borde de ataque | $w$ medio en el de salida |
|---|---|---|---|---|
| 0 (cubo) | 0,585 | 0,043 | 0,283 | 0,345 |
| 0,5 | 0,626 | 0,065 | 0,377 | 0,345 |
| 1 (carcasa) | 0,661 | 0,034 | 0,454 | 0,345 |

## Dónde se usa

- **Interfaz:**
  - vista *Curves*, con el inicio de la recirculación, $\psi_0$ y el caudal máximo
    ajustables;
  - opción *potential flow* de la vista meridional;
  - cuarta gráfica de la vista *Blade*;
  - secciones nuevas del informe.
- **Línea de comandos:** `turbopump performance`, que escribe la curva también en CSV, y
  `turbopump analysis`, con el flujo potencial y las caras del álabe, también en CSV.

## Limitaciones

- **Curva:**
  - el deslizamiento es constante con el caudal; la dependencia que muestra la estadística
    del libro solo está en una figura;
  - la potencia a válvula cerrada es una extrapolación.
- **NPSH:** solo se da en el punto de diseño; falta la curva NPSH₃(Q).
- **Flujo potencial:** no tiene en cuenta el giro ni el rozamiento, y no se usa todavía
  para construir los álabes; el diseño sigue con el flujo cuasi unidimensional.
- **Velocidades en las caras:** sin deslizamiento, la carga no se anula en el borde de
  salida como en el flujo real.
- **Pendiente:** el análisis álabe a álabe en dos dimensiones.
