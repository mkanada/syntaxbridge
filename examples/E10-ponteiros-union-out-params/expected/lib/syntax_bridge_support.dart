import 'dart:typed_data';

final class SyntaxBridgeOpaque {
  const SyntaxBridgeOpaque();
}

final class SyntaxBridgeByteCursor {
  SyntaxBridgeByteCursor(this._bytes, [this._offset = 0]);

  final Uint8List _bytes;
  int _offset;

  int operator [](int i) => _bytes[_offset + i];
  void operator []=(int i, int v) {
    _bytes[_offset + i] = v;
  }

  int get value => _bytes[_offset];
  set value(int v) {
    _bytes[_offset] = v;
  }

  SyntaxBridgeByteCursor operator +(int n) =>
      SyntaxBridgeByteCursor(_bytes, _offset + n);

  Object operator -(Object other) {
    if (other is int) {
      return SyntaxBridgeByteCursor(_bytes, _offset - other);
    }
    if (other is SyntaxBridgeByteCursor) {
      return _offset - other._offset;
    }
    throw ArgumentError('Invalid operand for -: $other');
  }

  int distanceTo(SyntaxBridgeByteCursor other) => other._offset - _offset;

  bool operator <(SyntaxBridgeByteCursor other) => _offset < other._offset;
  bool operator <=(SyntaxBridgeByteCursor other) => _offset <= other._offset;
  bool operator >(SyntaxBridgeByteCursor other) => _offset > other._offset;
  bool operator >=(SyntaxBridgeByteCursor other) => _offset >= other._offset;

  @override
  bool operator ==(Object other) =>
      other is SyntaxBridgeByteCursor &&
      identical(_bytes, other._bytes) &&
      _offset == other._offset;

  @override
  int get hashCode => Object.hash(identityHashCode(_bytes), _offset);
}
