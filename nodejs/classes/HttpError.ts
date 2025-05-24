class HttpError extends Error {
  status: number;

  /**
   * Create an HttpError.
   *
   * This class is used to create error objects that can be used to throw
   * HTTP errors in a typescript nodejs application. It is used to provide
   * a correct HTTP status code and a user-friendly error message.
   *
   * @param {String} message Error message
   * @param {Number} status HTTP status code
   */
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
    Object.setPrototypeOf(this, HttpError.prototype);
  }
}

export default HttpError;
