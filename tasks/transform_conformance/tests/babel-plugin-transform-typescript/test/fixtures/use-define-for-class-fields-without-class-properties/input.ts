class Cls {
	x: number;
	y = 1;
	@dce
	z;

	[x]: number;
	[y] = 1;
	@dce
	[z]: number;
}

class ClsWithConstructor {
  constructor() {
    console.log('constructor');
    super();
  }

  x: number;
  y = 1;
  @dce
  z;

  [x]: number;
  [y] = 1;
  @dce
  [z]: number;
}

class StaticCls {
	static x: number;
	static y = 1;
	@dce
	static z;

	static [x]: number;
	static [y] = 1;
	@dce
	static [z]: number;
}
