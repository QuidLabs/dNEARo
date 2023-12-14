
// erf(x): error function (see: https://en.wikipedia.org/wiki/Error_function)
// by https://github.com/jeremybarnes/cephes/blob/master/cprob/ndtr.c
const P = [
  2.46196981473530512524E-10,
  5.64189564831068821977E-1,
  7.46321056442269912687E0,
  4.86371970985681366614E1,
  1.96520832956077098242E2,
  5.26445194995477358631E2,
  9.34528527171957607540E2,
  1.02755188689515710272E3,
  5.57535335369399327526E2,
];
const Q = [
  1.32281951154744992508E1,
  8.67072140885989742329E1,
  3.54937778887819891062E2,
  9.75708501743205489753E2,
  1.82390916687909736289E3,
  2.24633760818710981792E3,
  1.65666309194161350182E3,
  5.57535340817727675546E2,
];
const R = [
  5.64189583547755073984E-1,
  1.27536670759978104416E0,
  5.01905042251180477414E0,
  6.16021097993053585195E0,
  7.40974269950448939160E0,
  2.97886665372100240670E0,
];
const S = [
  2.26052863220117276590E0,
  9.39603524938001434673E0,
  1.20489539808096656605E1,
  1.70814450747565897222E1,
  9.60896809063285878198E0,
  3.36907645100081516050E0,
];
const T = [
  9.60497373987051638749E0,
  9.00260197203842689217E1,
  2.23200534594684319226E3,
  7.00332514112805075473E3,
  5.55923013010394962768E4,
];
const U = [
  3.35617141647503099647E1,
  5.21357949780152679795E2,
  4.59432382970980127987E3,
  2.26290000613890934246E4,
  4.92673942608635921086E4,
];
const MAXLOG = Math.log(Number.MAX_VALUE)

function polevl(x, c) {
  return c.reduce((r, c) => r * x + c, 0)
}
function p1evl(x, c) {
  return c.reduce((r, c) => r * x + c, 1)
}

function erf(x) {
  if (Math.abs(x) > 1) return 1 - erfc(x)
  const z = x * x
  return x * polevl(z, T) / p1evl(z, U)
}

// erfc(x) = 1 - erf(x)
function erfc(x0) {
  const x = Math.abs(x0)
  if (x < 1) return 1 - erf(x)
  const z = -x0 * x0
  if (z < -MAXLOG) return x0 < 0 ? 2 : 0
  const [p, q] = x < 8 ? [P, Q] : [R, S]
  const y = Math.exp(z) * polevl(x, p) / p1evl(x, q)
  return x0 < 0 ? 2 - y : y
}

function RationalApproximation(t) {
  // Abramowitz and Stegun formula 26.2.23.
  // The absolute value of the error should be less than 4.5 e-4.
  let c = [2.515517, 0.802853, 0.010328]
  let d = [1.432788, 0.189269, 0.001308]
  return t - ((c[2] * t + c[1]) * t + c[0]) /
      (((d[2] * t + d[1]) * t + d[0]) * t + 1.0)
}

function NormalCDFInverse(p) {
  if (p < 0.5) { // F^-1(p) = -G^-1(p)
      let n = -2.0 * Math.log(p)
      return -1.0 * RationalApproximation(Math.sqrt(n))
  }
  else { // F^-1(p) = G^-1(1-p)
      let l = 1.0 - p
      let n = -2.0 * Math.log(l)
      return RationalApproximation(Math.sqrt(n))
  }
}

function stress(alpha, sqrt_var, short) { // max portfolio loss in %
  let sqrt2pi = Math.sqrt(2.0 * Math.PI)
  let d = NormalCDFInverse(alpha)
  let e1 = -1.0 * (d * d) / 2.0
  let e2
  if (short) {
    e2 = Math.exp(e1) / sqrt2pi / (1.0 - alpha) * sqrt_var
    return Math.exp(e2) - 1.0
  } else {
    e2 = -1.0 * (Math.exp(e1) / sqrt2pi / (1.0 - alpha) * sqrt_var)
    return -1.0 * (Math.exp(e2) - 1.0)
  }
}

// stress test the collateral portfolio of each user, as measured by CVaR
// record amount by which user's stressed collateral value would drop below user's debt
function stressPledge(p, print = false) {
  let vol1 = 0.4
  let price1 = 1
  //let vol2 = 0.9
  //let price2 = 2
  
  let iW = (p.coll[0] * price1)
  //let jW = (p.coll[1] * price2)
  let totalVal = iW //+ jW
  iW /= totalVal
  //jW /= totalVal

  let portVariance = 2.0 * iW * vol1 //* jW * vol2;
  portVariance += Math.pow(iW, 2) * Math.pow(vol1, 2);
  //portVariance += Math.pow(jW, 2) * Math.pow(vol2, 2);
  
  let vol = Math.sqrt(portVariance)

  let stresscol = stress(0.90, vol, false) 
  //console.log('stresscol...', stresscol)

  let svalueofcol = (1.0 - stresscol) * totalVal
  //console.log('svalueofcol...', svalueofcol)
  
  let svalueofcole = Math.max(p.debt - svalueofcol, 0.0)
  //console.log('svalueofcole...', svalueofcole)

  if (print) {
      console.log('...vol...', vol)
      console.log('stresscol...', stresscol) 
      console.log('svalueofcol...', svalueofcol)
      console.log('svalueofcole...', svalueofcole)  
  }
  return svalueofcole
}

function compareColl() {
    let trin = {
        debt: 1500,
        coll: [1200, 100],
    }
    let neo = {
        debt: 1234,
        coll: [1888, 42]
    }
    let morph = {
        debt: 666,
        coll: [888, 420]
    }    

    // --------------------------------------------------------------------------
    let pool = {
        debt: trin.debt + neo.debt + morph.debt,
        coll: [
                (trin.coll[0] + neo.coll[0] + morph.coll[0]),
                //(trin.coll[1] + neo.coll[1] + morph.coll[1])
              ],
    }
    let trinPool = {
        debt: trin.debt + neo.debt + morph.debt,
        coll: [
                (trin.coll[0] + neo.coll[0] + morph.coll[0]),
                //(trin.coll[1] + neo.coll[1] + morph.coll[1])
              ],
    }
    let morphPool = {
        debt: trin.debt + neo.debt + morph.debt,
        coll: [
                (trin.coll[0] + neo.coll[0] + morph.coll[0]),
                //(trin.coll[1] + neo.coll[1] + morph.coll[1])
              ],
    }
    let neoPool = {
        debt: trin.debt + neo.debt + morph.debt,
        coll: [
                (trin.coll[0] + neo.coll[0] + morph.coll[0]),
                //(trin.coll[1] + neo.coll[1] + morph.coll[1])
              ],
    }

    let total = stressPledge(pool)
    console.log('total', total)

    let tally = 0 // we are testing if the aggregated s_val_col_e equals 
    // from the one received at the bottom (where portfolio's stressed as a whole)
    let sum_deltas = 0
    // --------------------------------------------------------------------------
    
    trin['stress'] = stressPledge(trin)
    tally += trin['stress']
    console.log("trin['stress']...", trin['stress'])

    // remove user from pool
    trinPool['debt'] -= trin['debt']
    trinPool['coll'][0] -= trin['coll'][0]
    //trinPool['coll'][1] -= trin['coll'][1]

    trin['delta'] = total - stressPledge(trinPool)
    console.log("trin['delta']...", trin['delta'])
    sum_deltas += trin['delta'] 

    // --------------------------------------------------------------------------
    
    neo['stress'] = stressPledge(neo)
    tally += neo['stress']
    console.log("neo['stress']...", neo['stress'])
    // remove user from pool
    neoPool['debt'] -= neo['debt']
    neoPool['coll'][0] -= neo['coll'][0]
    //neoPool['coll'][1] -= neo['coll'][1]

    neo['delta'] = total - stressPledge(neoPool)
    console.log("neo['delta']...", neo['delta'])
    sum_deltas += neo['delta'] 

    // --------------------------------------------------------------------------

    morph['stress'] = stressPledge(morph)
    tally += morph['stress']
    console.log("morph['stress']...", morph['stress'])
    
    // remove user from pool
    morphPool['debt'] -= morph['debt']
    morphPool['coll'][0] -= morph['coll'][0]
    //morphPool['coll'][1] -= morph['coll'][1]

    morph['delta'] = total - stressPledge(morphPool)
    console.log("morph['delta']...", morph['delta'])
    sum_deltas += morph['delta'] 

    console.log('tally', tally)    
    console.log('sum_deltas', sum_deltas)    
}

compareColl()