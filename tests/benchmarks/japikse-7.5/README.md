# Ejemplo de diseño de Japikse (cap. 7.5)

**Fuente:** D. Japikse, W. D. Marscher y R. B. Furst, *Centrifugal Pump Design and
Performance*, Concepts ETI, 1997, cap. 7.5, Fig. 7.73 (pp. 7-74 a 7-78). El número de álabes
y el ángulo de salida final están en la p. 7-86.

Es una bomba de agua de 5000 gpm (0,315 m³/s), 84,05 ft (25,6 m) y 1000 rpm, con
$N_s$ (EE. UU.) = 2546, es decir $n_q$ ≈ 49. Tiene un rodete de 7 álabes, un difusor sin
álabes hasta $R_5$ = 12 in y una voluta.

## Entradas

Del libro se toman:

- el punto de diseño y el agua a 100 °F;
- el rodete: $r_2$ = 8,8208 in, $b_2$ = 3,1544 in, $r_{1t}$ = 6,2178 in y $r_{1h}$ = 3,3750 in
  (es decir, $d_1$ y el cubo), y $z$ = 7;
- el difusor sin álabes, de $r_2$ a 12 in, y el ancho de entrada de la voluta,
  $B_5$ = 3,1304 in.

$\beta_{2B}$ no se fija: Turbopump lo despeja para dar la altura con su deslizamiento. Lo
demás es automático.

Para rehacer `caso.tpj` (la temperatura de entrada se fija editando
`setup.inlet.total_temperature` = 310,928 K):

```bash
turbopump new caso.tpj --name "Japikse 7.5" --machine pump --flow "5000 gpm" --head "84.05 ft" --speed "1000 rpm" --components impeller,vaneless-stator,volute
```

```bash
turbopump design caso.tpj --set "d2=17.6416 in" --set "b2=3.1544 in" --set "d1=12.4356 in" --set "d-hub=6.75 in" --set z=7
```

```bash
turbopump diffuser caso.tpj --set "d3=17.6416 in" --set "b3=3.1544 in" --set "d4=24 in" --set "b4=3.1304 in"
```

```bash
turbopump volute caso.tpj --set "b3=3.1304 in"
```

## Resultado

```bash
turbopump validate caso.tpj targets.json
```

| Magnitud | Libro | Turbopump | Δ | Tolerancia | |
|---|---|---|---|---|---|
| $n_q$ | 49,29 | 49,32 | +0,1 % | ±1 % | dentro |
| $c_{u2}$ | 39,89 ft/s | 37,44 ft/s | −6,1 % | ±3 % | **fuera** |
| $c_{m2}$ | 9,222 ft/s | 9,460 ft/s | +2,6 % | ±5 % | dentro |
| $\alpha_2$ | 13,0° | 14,2° | +1,2° | ±1° | **fuera** |
| $\beta_2$ | 14,0° | 13,5° | −0,5° | ±1° | dentro |
| $\beta_{2B}$ | 22,5° | 21,75° | −0,75° | ±1° | dentro |
| Deslizamiento | 0,807 | 0,827 | +2,5 % | — | informativa |
| $c_{m1}$ | 12,47 ft/s | 19,30 ft/s | — | — | informativa |
| $w_{1}$ en la carcasa | 55,88 ft/s | 57,59 ft/s | +3,1 % | — | informativa |
| $\beta_1$ en la carcasa | 13,8° | 19,6° | — | — | informativa |
| $\eta$ de la etapa | 81,8 % | 89,1 % | +7,3 pt | ±2 pt | **fuera** |
| Potencia | 130,3 hp | 118,4 hp | −9,1 % | ±3 % | **fuera** |
| λ a la entrada de la voluta | 4,18 | 4,11 | −1,7 % | ±5 % | dentro |
| Coeficiente de pérdidas de la espiral | 0,054 | 0,056 | +0,002 | ±0,003 | dentro |
| NPSH | 12,47 ft | 13,00 ft | +4,2 % | — | informativa |

## Lectura

- **Rendimiento y potencia.** Turbopump toma el rendimiento de la estadística de Gülich
  (tabla 3.9), que a $n_q$ = 49 y 0,315 m³/s da 89 %. El libro suma sus pérdidas una a
  una, y entre ellas hay 3,5 puntos por la fuga del anillo delantero y una potencia de
  recirculación que el autor fija, a propósito conservadora, en el 2 % de la de Euler. La
  diferencia es de modelo y no se corrige aquí.
- **$c_{u2}$ y $\alpha_2$.** Ambos siguen de lo anterior. Turbopump calcula $c_{u2}$ con
  $H_{th} = H/\eta_h$, con el $\eta_h$ de Gülich; el libro, con su modelo de dos zonas
  (chorro y estela) y una desviación del chorro que elige el diseñador. El ángulo del
  álabe que hace falta para la altura coincide dentro de 0,75°.
- **Entrada.** La estación 1 del libro está sobre un borde de ataque inclinado
  (PHI1 = 40°), donde el área es 1,5 veces la del ojo. Turbopump calcula su triángulo en
  el plano del ojo, así que $c_{m1}$ y los ángulos de entrada no son la misma magnitud.
  Por eso pasaron a informativas tras la primera ejecución.
- **Voluta.** Con el difusor y el ancho de entrada del libro, el parámetro de giro y las
  pérdidas de la espiral (Japikse cap. 4.3) coinciden.
