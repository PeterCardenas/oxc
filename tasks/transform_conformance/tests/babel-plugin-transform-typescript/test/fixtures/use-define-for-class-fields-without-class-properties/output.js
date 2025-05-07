
let _y, _y2;
class Cls {
  constructor() {
    this.y = 1;
    this[_y] = 1;
  }
  static {
    _y = y;
  }
}

class ClsWithConstructor {
  static {
    _y2 = y;
  }
  constructor() {
    console.log("constructor");
    super();
    this.y = 1;
    this[_y2] = 1;
  }
}

class StaticCls {
  static y = 1;
  static [y] = 1;
}
