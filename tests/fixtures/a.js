// A plain function.
function greet(name) {
  return `hi ${name}`;
}

const shout = (s) => s.toUpperCase();

const legacy = function (x) {
  return x;
};

class Counter {
  constructor() {
    this.n = 0;
  }
  inc() {
    this.n += 1;
  }
}

module.exports = { greet, shout, legacy, Counter };
